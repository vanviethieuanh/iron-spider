use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam::channel::{Receiver, Sender};
use tracing::{error, info, warn};

use crate::{
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
}

pub struct SpiderManager {
    pending_spiders: Mutex<VecDeque<u64>>,
    registered_spiders: Vec<RegisteredSpider>,
    id_map: HashMap<u64, usize>,
    stats_tracker: Arc<SpiderManagerStatsTracker>,
}

impl SpiderManager {
    pub fn new(spiders: Vec<Arc<dyn Spider>>) -> Self {
        let start_spider_count = spiders.len();
        let mut registered_spiders = Vec::with_capacity(spiders.len());
        let mut id_map = HashMap::with_capacity(spiders.len());
        let stats_tracker = Arc::new(SpiderManagerStatsTracker::new(start_spider_count));

        let mut pending_spiders = VecDeque::new();
        for spider in spiders {
            let registered = RegisteredSpider::new(spider);
            let registered_id = registered.id();

            id_map.insert(registered_id, registered_spiders.len());
            registered_spiders.push(registered);
            pending_spiders.push_back(registered_id);
        }

        Self {
            pending_spiders: Mutex::new(pending_spiders),
            registered_spiders,
            id_map,
            stats_tracker,
        }
    }

    fn deactivate_spider(&self, spider_id: u64) {
        if let Some(&index) = self.id_map.get(&spider_id) {
            let registered_spider = &self.registered_spiders[index];
            let state = &registered_spider.state;

            if state.is_activated() {
                warn!(
                    "Spider '{} - {}' is forced to close.",
                    registered_spider.inner.name(),
                    registered_spider.id
                );
            }

            self.stats_tracker.deactivate_one_spider();

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

    fn activate_spider(&self, spider_id: &u64, num_requests: usize) {
        if let Some(&index) = self.id_map.get(&spider_id) {
            let state = &self.registered_spiders[index].state;

            state
                .in_flight_requests
                .fetch_add(num_requests, Ordering::Relaxed);
            state
                .created_requests
                .fetch_add(num_requests, Ordering::Relaxed);

            if state.is_activated() {
                self.stats_tracker.activate_one_spider();
            }
        }
    }

    pub fn get_stats(&self) -> SpiderManagerStats {
        self.stats_tracker
            .get_stats(self.pending_spiders.lock().unwrap().len())
    }

    fn handle_requests_result(
        &self,
        requests: Vec<Request>,
        spider: &Arc<RegisteredSpider>,
        scheduler: &Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
    ) {
        let mut sched = scheduler.lock().unwrap();
        let mut queued_requests = 0;

        for request in requests {
            let iron_request = IronRequest {
                registered_spider: Arc::clone(&spider),
                request,
            };
            match sched.enqueue(iron_request) {
                Ok(_) => queued_requests += 1,
                Err(e) => error!("Failed to enqueue request: {:?}", e),
            }
        }

        if queued_requests > 0 {
            spider
                .state
                .in_flight_requests
                .fetch_add(queued_requests, Ordering::Relaxed);
            spider
                .state
                .created_requests
                .fetch_add(queued_requests, Ordering::Relaxed);
        }
    }

    fn handle_items_result(&self, items: Vec<ResultItem>, item_sender: &Sender<ResultItem>) {
        for item in items {
            if item_sender.send(item).is_err() {
                break;
            }
        }
    }

    pub fn start(
        &self,
        scheduler: Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
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

                    let spider_result = response
                        .request
                        .registered_spider
                        .inner
                        .parse(response.clone());
                    response
                        .request
                        .registered_spider
                        .state
                        .in_flight_requests
                        .fetch_sub(1, Ordering::Relaxed);

                    let registered_spider = Arc::clone(&response.request.registered_spider);
                    match spider_result {
                        SpiderResult::Requests(requests) => {
                            self.handle_requests_result(requests, &registered_spider, &scheduler);
                        }
                        SpiderResult::Items(items) => {
                            self.handle_items_result(items, &item_sender);
                        }
                        SpiderResult::Both { requests, items } => {
                            self.handle_requests_result(requests, &registered_spider, &scheduler);
                            self.handle_items_result(items, &item_sender);
                        }
                        SpiderResult::None => {
                            self.stats_tracker.drop_one_response();
                        }
                    }

                    if !registered_spider.state.is_activated() {
                        self.deactivate_spider(registered_spider.id);
                    }
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }

        info!("🕷️  Spider Manager thread stopped");
        Ok(())
    }

    fn try_activate_pending_spider(
        &self,
        scheduler: &Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
    ) -> Result<(), EngineError> {
        let mut sched = scheduler.lock().unwrap();
        let pending_request_count = sched.count();
        if pending_request_count > SCHEDULER_HOLDING_THRESOLD {
            return Ok(());
        }

        let registered_spider_id = {
            let mut pending_spiders = self.pending_spiders.lock().unwrap();
            match pending_spiders.pop_front() {
                Some(spider) => spider,
                None => return Ok(()),
            }
        };

        let idx = self.id_map[&registered_spider_id];
        let registered_spider = &self.registered_spiders[idx];

        let start_requests = registered_spider.inner.start_requests();
        let start_requests_count = start_requests.len();
        let is_active = start_requests_count > 0;

        for request in start_requests {
            let idx = self.id_map[&registered_spider_id];
            let registered = &self.registered_spiders[idx];

            sched
                .enqueue(registered.make_request(request))
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
}
