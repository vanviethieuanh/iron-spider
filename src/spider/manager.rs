use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam::{
    channel::{Receiver, Sender},
    queue::ArrayQueue,
};
use dashmap::{DashMap, DashSet};
use rayon::{ThreadPool, ThreadPoolBuilder};
use tracing::{debug, error, info, warn};

use crate::{
    config::EngineConfig,
    errors::EngineError,
    item::ResultItem,
    request::{IronRequest, Request},
    response::Response,
    scheduler::scheduler::Scheduler,
    spider::{
        spider::{Spider, SpiderResult, SpiderState},
        stat::{SpiderManagerStats, SpiderManagerStatsTracker},
    },
};

static SPIDER_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

// When Scheuduler hold more than this amount,
// Spider Manager will stop activate new spider.
static SCHEDULER_HOLDING_THRESOLD: u64 = 50;

#[derive(Debug, Clone)]
pub struct RegisteredSpider {
    id: u64,
    inner: Arc<dyn Spider>,
    state: Arc<SpiderState>,
}

impl RegisteredSpider {
    pub fn new(inner: Arc<dyn Spider>) -> Self {
        let id = SPIDER_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            id,
            inner,
            state: Arc::new(SpiderState::new()),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn inner(&self) -> &dyn Spider {
        &*self.inner
    }

    pub fn make_request(&self, request: Request) -> IronRequest {
        IronRequest {
            registered_spider: Arc::new(self.clone()),
            request,
        }
    }

    pub fn request_started(&self, num: usize) {
        self.state
            .in_flight_requests
            .fetch_add(num, Ordering::Relaxed);
        self.state
            .created_requests
            .fetch_add(num, Ordering::Relaxed);
    }

    pub fn request_finished(&self) {
        self.state
            .in_flight_requests
            .fetch_sub(1, Ordering::Relaxed);
        debug!(
            "Done 1 request, in flight: {} - {}",
            self.state.in_flight_requests.load(Ordering::Relaxed),
            self.is_activated()
        );
    }

    pub fn is_activated(&self) -> bool {
        self.state.is_activated()
    }
}

pub struct SpiderManager {
    pending_spiders: ArrayQueue<u64>,
    registered_spiders: Arc<DashMap<u64, RegisteredSpider>>,
    working_spiders: Arc<DashSet<u64>>,
    stats_tracker: Arc<SpiderManagerStatsTracker>,
    thread_pool: ThreadPool,
}

impl SpiderManager {
    pub fn new(spiders: Vec<Arc<dyn Spider>>, config: &EngineConfig) -> Self {
        let start_spider_count = spiders.len();
        let registered_spiders = Arc::new(DashMap::with_capacity(spiders.len()));
        let stats_tracker = Arc::new(SpiderManagerStatsTracker::new(start_spider_count));
        let working_spiders = Arc::new(DashSet::new());

        let pending_spiders = ArrayQueue::new(spiders.len());
        for spider in spiders {
            let registered = RegisteredSpider::new(spider);
            let registered_id = registered.id();

            registered_spiders.insert(registered_id, registered);
            let _ = pending_spiders.push(registered_id);
        }

        Self {
            pending_spiders,
            registered_spiders,
            stats_tracker,
            working_spiders,
            thread_pool: ThreadPoolBuilder::new()
                .num_threads(config.spider_manager_worker_threads)
                .build()
                .unwrap(),
        }
    }

    fn activate_spider(&self, spider_id: &u64, num_requests: usize) {
        if let Some(registered_spiders) = &self.registered_spiders.get(&spider_id) {
            registered_spiders.request_started(num_requests);
            if registered_spiders.is_activated() {
                self.stats_tracker.activate_one_spider();
                self.working_spiders.insert(*spider_id);
            }
        }
    }

    pub fn get_stats(&self) -> SpiderManagerStats {
        self.stats_tracker.get_stats(self.pending_spiders.len())
    }

    pub fn start(
        &self,
        scheduler: Arc<dyn Scheduler>,
        resp_receiver: Receiver<Response>,
        item_sender: Sender<ResultItem>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<std::sync::Mutex<Instant>>,
    ) -> Result<(), EngineError> {
        while !shutdown_signal.load(Ordering::Relaxed) {
            let _ = self.try_activate_pending_spider(&scheduler);

            match resp_receiver.try_recv() {
                Ok(response) => {
                    *last_activity.lock().unwrap() = Instant::now();

                    let item_sender = item_sender.clone();
                    let scheduler = Arc::clone(&scheduler);
                    let stats_tracker = Arc::clone(&self.stats_tracker);

                    self.thread_pool.spawn_fifo(move || {
                        stats_tracker.parse_thread_started();
                        let spider_result = response
                            .request
                            .registered_spider
                            .inner
                            .parse(response.clone());

                        info!("Got result");

                        Self::handler_spider_result(
                            response,
                            item_sender,
                            scheduler,
                            &stats_tracker,
                            spider_result,
                        );

                        info!("Result handled");

                        stats_tracker.parse_thread_finished();

                        info!("tracked");
                    });
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }

            for spider_entry in self.registered_spiders.iter() {
                let id = *spider_entry.key();
                let spider = spider_entry.value();
                if !spider.state.is_activated() && self.working_spiders.contains(&id) {
                    Self::deactivate_spider_static(
                        &id,
                        &self.registered_spiders,
                        &self.stats_tracker,
                        &self.working_spiders,
                    );
                }
            }
        }

        self.close();

        info!("🕷️  Spider Manager thread stopped");
        Ok(())
    }

    fn close(&self) {
        info!("Spider Manager closing....");

        let spider_ids: Vec<u64> = self.working_spiders.iter().map(|id| *id).collect();

        let stats_tracker = &self.stats_tracker;
        let registered_spiders = &self.registered_spiders;
        let working_spiders = &self.working_spiders;

        self.thread_pool.scope(|s| {
            for working_spider_id in &spider_ids {
                s.spawn(move |_| {
                    Self::deactivate_spider_static(
                        &working_spider_id,
                        registered_spiders,
                        stats_tracker,
                        working_spiders,
                    );
                });
            }
        });

        // Wait for all parse threads to finish
        info!("Waiting for parse threads to finish...");
        while stats_tracker.any_parse_threads() {
            thread::sleep(Duration::from_millis(100));
        }

        info!("All working spiders have been deactivated.");
    }

    fn try_activate_pending_spider(
        &self,
        scheduler: &Arc<dyn Scheduler>,
    ) -> Result<(), EngineError> {
        let pending_request_count = scheduler.count();
        if pending_request_count > SCHEDULER_HOLDING_THRESOLD {
            return Ok(());
        }

        let registered_spider_id = {
            match self.pending_spiders.pop() {
                Some(spider) => spider,
                None => return Ok(()),
            }
        };

        match self.registered_spiders.get(&registered_spider_id) {
            Some(registered_spider) => {
                let start_requests = registered_spider.inner.start_requests();
                let start_requests_count = start_requests.len();
                let is_active = start_requests_count > 0;

                for request in start_requests {
                    scheduler
                        .enqueue(registered_spider.make_request(request))
                        .map_err(|e| {
                            EngineError::InitializationError(format!(
                                "Failed to seed request(s) of spider #{}, error: {:?}",
                                &registered_spider_id, e
                            ))
                        })?;
                }

                if is_active {
                    self.activate_spider(&registered_spider_id, start_requests_count);
                }
                Ok(())
            }
            None => Ok(()),
        }
    }
}

impl SpiderManager {
    fn handler_spider_result(
        response: Response,
        item_sender: Sender<ResultItem>,
        scheduler: Arc<dyn Scheduler + 'static>,
        stats_tracker: &Arc<SpiderManagerStatsTracker>,
        spider_result: SpiderResult,
    ) {
        let registered_spider = Arc::clone(&response.request.registered_spider);

        match spider_result {
            SpiderResult::Requests(requests) => {
                SpiderManager::handle_requests_result(requests, &registered_spider, &scheduler);
            }
            SpiderResult::Items(items) => {
                SpiderManager::handle_items_result(items, &item_sender);
            }
            SpiderResult::Both { requests, items } => {
                SpiderManager::handle_requests_result(requests, &registered_spider, &scheduler);
                SpiderManager::handle_items_result(items, &item_sender);
            }
            SpiderResult::None => {
                stats_tracker.drop_one_response();
            }
        }
    }

    fn handle_items_result(items: Vec<ResultItem>, item_sender: &Sender<ResultItem>) {
        for item in items {
            if item_sender.send(item).is_err() {
                break;
            }
        }
    }

    fn handle_requests_result(
        requests: Vec<Request>,
        registered_spider: &Arc<RegisteredSpider>,
        scheduler: &Arc<dyn Scheduler>,
    ) {
        let mut queued_requests = 0;

        for request in requests {
            let iron_request = IronRequest {
                registered_spider: Arc::clone(&registered_spider),
                request,
            };
            match scheduler.enqueue(iron_request) {
                Ok(_) => queued_requests += 1,
                Err(e) => error!("Failed to enqueue request: {:?}", e),
            }
        }

        if queued_requests > 0 {
            registered_spider.request_started(queued_requests);
        }
    }

    fn deactivate_spider_static(
        spider_id: &u64,
        registered_spiders: &DashMap<u64, RegisteredSpider>,
        stats_tracker: &Arc<SpiderManagerStatsTracker>,
        working_spiders: &DashSet<u64>,
    ) {
        if let Some(registered_spider) = registered_spiders.get(&spider_id) {
            let state = &registered_spider.state;

            if state.is_activated() {
                warn!(
                    "Spider '{} - {}' is forced to close.",
                    registered_spider.inner.name(),
                    registered_spider.id
                );
            }

            stats_tracker.deactivate_one_spider();
            working_spiders.remove(&spider_id);

            registered_spider.inner.close();
            info!(
                "Spider '{} - {}' has closed.",
                registered_spider.inner.name(),
                registered_spider.id
            );
        } else {
            error!("⚠️ Tried to deactivate unknown spider_id: {}", spider_id);
        }
    }
}
