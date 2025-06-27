use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
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
    scheduler::Scheduler,
    spider::spider::{Spider, SpiderResult, SpiderState},
};

static SPIDER_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

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

pub struct SpiderManagerStats {
    pub dropped_responses: usize,
    pub total_spiders: usize,
    pub sleeping_spiders: usize,
    pub active_spiders: usize,
}

pub struct SpiderManager {
    registered_spiders: Vec<RegisteredSpider>,
    id_map: HashMap<u64, usize>,

    spider_counts: usize,
    active_spiders: AtomicUsize,
    dropped_responses: AtomicUsize,
}

impl SpiderManager {
    pub fn new(spiders: Vec<Arc<dyn Spider>>) -> Self {
        let start_spider_count = spiders.len();
        let mut registered_spiders = Vec::with_capacity(spiders.len());
        let mut id_map = HashMap::with_capacity(spiders.len());

        for spider in spiders {
            let registered = RegisteredSpider::new(spider);
            id_map.insert(registered.id(), registered_spiders.len());
            registered_spiders.push(registered);
        }

        Self {
            registered_spiders,
            id_map,

            spider_counts: start_spider_count,
            active_spiders: AtomicUsize::new(0),
            dropped_responses: AtomicUsize::new(0),
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

            self.active_spiders.fetch_sub(1, Ordering::SeqCst);
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

    fn activate_spider(&self, spider_id: u64, num_requests: usize) {
        if let Some(&index) = self.id_map.get(&spider_id) {
            let state = &self.registered_spiders[index].state;

            state
                .in_flight_requests
                .fetch_add(num_requests, Ordering::SeqCst);
            state
                .created_requests
                .fetch_add(num_requests, Ordering::Relaxed);

            if !state.is_activated() {
                self.active_spiders.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    pub fn get_stats(&self) -> SpiderManagerStats {
        let active = self.active_spiders.load(Ordering::Relaxed);
        let dropped = self.dropped_responses.load(Ordering::Relaxed);

        SpiderManagerStats {
            dropped_responses: dropped,
            total_spiders: self.spider_counts,
            active_spiders: active,
            sleeping_spiders: self.spider_counts - active,
        }
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
        self.seed_initial_requests(scheduler.clone())?;

        while !shutdown_signal.load(Ordering::Relaxed) {
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
                            // No action needed
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

        println!("🕷️  Spider Manager thread stopped");
        Ok(())
    }

    fn seed_initial_requests(
        &self,
        scheduler: Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
    ) -> Result<(), EngineError> {
        println!("🌱 Seeding initial requests to scheduler...");

        let mut sched = scheduler.lock().unwrap();
        let mut start_requests_count = 0;

        // Get start requests from each spider
        for registered_spiders in &self.registered_spiders {
            let spider = registered_spiders.inner();
            let start_requests = spider.start_requests();

            let created_requests = start_requests.len();
            let is_active = created_requests > 0;

            for request in start_requests {
                sched
                    .enqueue(registered_spiders.make_request(request))
                    .map_err(|e| {
                        EngineError::InitializationError(format!(
                            "Failed to seed request(s) of spider #{}, error: {:?}",
                            registered_spiders.id, e
                        ))
                    })?;
                start_requests_count += 1;
            }

            if is_active {
                self.activate_spider(registered_spiders.id, created_requests);
            }
        }

        println!("🌱 Seeded {} initial requests", start_requests_count);
        Ok(())
    }
}
