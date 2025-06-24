use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam::channel::{Receiver, Sender, unbounded};
use futures::future::join_all;
use reqwest::Client;
use tokio::{runtime::Runtime, task::JoinSet};
use tracing::{debug, error, info, warn};

use crate::{
    config::Configuration,
    downloader::Downloader,
    pipeline::PipelineManager,
    request::Request,
    response::Response,
    scheduler::{Scheduler, SimpleScheduler},
    spider::{ResultItem, Spider, SpiderResult},
};

pub struct Engine {
    scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
    spiders: Vec<Box<dyn Spider>>,
    pipeline_manager: PipelineManager,
    config: Configuration,
    downloader: Arc<Downloader>,
    tasks: JoinSet<()>,

    active_requests: Arc<AtomicUsize>,
    shutdown_signal: Arc<AtomicBool>,
    last_activity: Arc<std::sync::Mutex<Instant>>,
}

impl Engine {
    pub fn new(
        scheduler: Box<dyn Scheduler + Send + Sync>,
        spiders: Vec<Box<dyn Spider>>,
        pipelines: PipelineManager,
        config: Option<Configuration>,
    ) -> Self {
        let config = config.unwrap_or_default();

        let mut client_builder = Client::builder()
            .timeout(config.downloader_request_timeout)
            .connector_layer(tower::limit::concurrency::ConcurrencyLimitLayer::new(
                config.concurrent_limit,
            ));

        if let Some(ref user_agent) = config.user_agent {
            client_builder = client_builder.user_agent(user_agent);
        }

        let downloader_client = client_builder
            .build()
            .expect("Failed to build downloader's client.");

        let downloader_request_quota = config.downloader_request_quota;
        let http_error_allow_codes = config.http_error_allow_codes.clone();

        Self {
            scheduler: Arc::new(Mutex::new(scheduler)),
            spiders,
            pipeline_manager: pipelines,
            config,
            downloader: Arc::new(Downloader::new(
                downloader_client,
                downloader_request_quota,
                http_error_allow_codes,
            )),
            tasks: JoinSet::new(),
            active_requests: Arc::new(AtomicUsize::new(0)),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            last_activity: Arc::new(std::sync::Mutex::new(Instant::now())),
        }
    }

    pub fn start(&mut self) {
        info!(
            "🕷️ Starting Iron-Spider Engine {} spiders",
            self.spiders.len()
        );

        // Create communication channels
        let (resp_sender, resp_receiver) = unbounded::<Response>();
        let (item_sender, item_receiver) = unbounded::<ResultItem>();

        // Create shared state
        let active_requests = Arc::clone(&self.active_requests);
        let shutdown_signal = Arc::clone(&self.shutdown_signal);
        let last_activity = Arc::clone(&self.last_activity);

        // Use crossbeam::scope for structured concurrency
        crossbeam::scope(|scope| {
            // 1. Spawn Downloader Threads
            // Downloader pulls requests directly from scheduler
            let downloader_handles: Vec<_> = (0..self.config.downloader_threads)
                .map(|i| {
                    let scheduler = Arc::clone(&self.scheduler);
                    let resp_sender = resp_sender.clone();
                    let active_requests = Arc::clone(&active_requests);
                    let shutdown_signal = Arc::clone(&shutdown_signal);
                    let last_activity = Arc::clone(&last_activity);

                    scope.spawn(move |_| {
                        println!("⬇️  Downloader thread {} started", i);
                        self.run_downloader(
                            i as usize,
                            resp_sender,
                            active_requests,
                            shutdown_signal,
                            last_activity,
                        )
                    })
                })
                .collect();

            // 2. Spawn Spider Manager Thread
            // Spider Manager pushes requests to scheduler
            let spider_handle = {
                let resp_receiver = resp_receiver.clone();
                let item_sender = item_sender.clone();
                let active_requests = Arc::clone(&active_requests);
                let shutdown_signal = Arc::clone(&shutdown_signal);
                let last_activity = Arc::clone(&last_activity);
                let spiders = std::mem::take(&mut self.spiders);

                scope.spawn(move |_| {
                    println!("🕷️  Spider Manager thread started");
                    self.run_spider_manager(
                        spiders,
                        resp_receiver,
                        item_sender,
                        active_requests,
                        shutdown_signal,
                        last_activity,
                    )
                })
            };

            // 3. Pipeline Manager threads (unchanged)
            let pipeline_handles: Vec<_> = (0..self.config.pipeline_worker_threads)
                .map(|i| {
                    let item_receiver = item_receiver.clone();
                    let shutdown_signal = Arc::clone(&shutdown_signal);
                    let pipeline_manager = self.pipeline_manager.clone();

                    scope.spawn(move |_| {
                        println!("🔧 Pipeline Manager thread {} started", i);
                        self.run_pipeline_manager(
                            i as usize,
                            pipeline_manager,
                            item_receiver,
                            shutdown_signal,
                        )
                    })
                })
                .collect();

            // 5. Spawn Health Check & Stats Thread
            let health_handle = {
                let active_requests = Arc::clone(&active_requests);
                let shutdown_signal = Arc::clone(&shutdown_signal);
                let last_activity = Arc::clone(&last_activity);
                let config = self.config.clone();

                scope.spawn(move |_| {
                    println!("💊 Health check thread started");
                    self.run_health_check(active_requests, shutdown_signal, last_activity, config)
                })
            };

            // 6. Seed initial requests
            self.seed_initial_requests(&req_sender)?;

            println!("🚀 All threads spawned, engine running...");

            // Wait for all threads to complete
            // The health check thread will signal shutdown when appropriate
            let _ = health_handle.join();
            println!("🛑 Shutdown signal received, waiting for threads to finish...");

            // Wait for all worker threads with timeout
            let shutdown_start = Instant::now();
            while shutdown_start.elapsed() < self.config.shutdown_timeout {
                if scheduler_handle.is_finished()
                    && downloader_handles.iter().all(|h| h.is_finished())
                    && spider_handle.is_finished()
                    && pipeline_handles.iter().all(|h| h.is_finished())
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            println!("✅ Iron-Spider Engine stopped");
            Ok(())
        })
        .unwrap();

        info!("Engine finished crawling.");
    }

    fn run_downloader(
        &self,
        thread_id: usize,
        resp_sender: Sender<Response>,
        active_requests: Arc<AtomicUsize>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<std::sync::Mutex<Instant>>,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

        rt.block_on(async {
            let client = reqwest::Client::new();

            while !shutdown_signal.load(Ordering::Relaxed) {
                // Pull request from scheduler
                let request = {
                    let mut sched = self.scheduler.lock().unwrap();
                    sched.dequeue()
                };

                match request {
                    Some(req) => {
                        active_requests.fetch_add(1, Ordering::Relaxed);
                        *last_activity.lock().unwrap() = Instant::now();

                        // Perform HTTP request
                        match self.perform_request(&client, req).await {
                            Ok(response) => {
                                if resp_sender.send(response).is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                eprintln!("Request failed: {}", e);
                            }
                        }

                        active_requests.fetch_sub(1, Ordering::Relaxed);
                    }
                    None => {
                        // No requests available, sleep briefly
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }
        });

        println!("⬇️  Downloader thread {} stopped", thread_id);
    }

    // CORRECTED: Spider Manager pushes to scheduler
    fn run_spider_manager(
        &self,
        spiders: Vec<Box<dyn Spider>>,
        resp_receiver: Receiver<Response>,
        item_sender: Sender<ResultItem>,
        active_requests: Arc<AtomicUsize>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<std::sync::Mutex<Instant>>,
    ) {
        while !shutdown_signal.load(Ordering::Relaxed) {
            match resp_receiver.try_recv() {
                Ok(response) => {
                    *last_activity.lock().unwrap() = Instant::now();

                    // Process response with appropriate spider
                    // This generates SpiderResult which can contain new requests
                    let spider_results = self.process_response_with_spiders(&spiders, response);

                    for result in spider_results {
                        match result {
                            SpiderResult::Requests(requests) => {
                                // Push new requests to scheduler
                                let mut sched = self.scheduler.lock().unwrap();
                                for request in requests {
                                    if let Err(e) = sched.enqueue(request) {
                                        eprintln!("Failed to enqueue request: {:?}", e);
                                    }
                                }
                            }
                            SpiderResult::Items(items) => {
                                // Send items to pipeline
                                for item in items {
                                    if item_sender.send(item).is_err() {
                                        break;
                                    }
                                }
                            }
                            SpiderResult::Both { requests, items } => {
                                // Handle both requests and items
                                let mut sched = self.scheduler.lock().unwrap();
                                for request in requests {
                                    if let Err(e) = sched.enqueue(request) {
                                        eprintln!("Failed to enqueue request: {:?}", e);
                                    }
                                }
                                drop(sched); // Release lock before sending items

                                for item in items {
                                    if item_sender.send(item).is_err() {
                                        break;
                                    }
                                }
                            }
                            SpiderResult::None => {
                                // No action needed
                            }
                        }
                    }
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }

        println!("🕷️  Spider Manager thread stopped");
    }

    fn run_pipeline_manager(
        &self,
        thread_id: usize,
        pipeline_manager: PipelineManager,
        item_receiver: Receiver<ResultItem>,
        shutdown_signal: Arc<AtomicBool>,
    ) {
        while !shutdown_signal.load(Ordering::Relaxed) {
            match item_receiver.try_recv() {
                Ok(item) => {
                    // Process item through pipeline
                    pipeline_manager.process_item(item);
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }

        println!("🔧 Pipeline Manager thread {} stopped", thread_id);
    }

    // CORRECTED: Health check includes scheduler state
    fn run_health_check(
        &self,
        scheduler: Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
        active_requests: Arc<AtomicUsize>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<std::sync::Mutex<Instant>>,
        config: Configuration,
    ) {
        let mut stats_timer = Instant::now();

        loop {
            std::thread::sleep(Duration::from_secs(1));

            // Print stats periodically
            if stats_timer.elapsed() >= config.stats_interval {
                let active = active_requests.load(Ordering::Relaxed);
                let scheduler_empty = {
                    let sched = scheduler.lock().unwrap();
                    sched.is_empty()
                };
                println!(
                    "📊 Active requests: {}, Scheduler empty: {}",
                    active, scheduler_empty
                );
                stats_timer = Instant::now();
            }

            // Check for shutdown conditions
            let last_activity_time = *last_activity.lock().unwrap();
            let idle_time = last_activity_time.elapsed();
            let active = active_requests.load(Ordering::Relaxed);
            let scheduler_empty = {
                let sched = scheduler.lock().unwrap();
                sched.is_empty()
            };

            // Shutdown when: no active requests AND scheduler is empty AND idle timeout
            if active == 0 && scheduler_empty && idle_time >= config.idle_timeout {
                println!("⏰ All work completed, initiating shutdown...");
                shutdown_signal.store(true, Ordering::Relaxed);
                break;
            }

            if shutdown_signal.load(Ordering::Relaxed) {
                break;
            }
        }

        println!("💊 Health check thread stopped");
    }

    fn seed_initial_requests_to_scheduler(
        &self,
        scheduler: &Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
    ) -> Result<(), EngineError> {
        println!("🌱 Seeding initial requests to scheduler...");

        let mut sched = scheduler.lock().unwrap();

        // Get start requests from each spider
        for spider in &self.spiders {
            let start_requests = spider.start_requests();
            for request in start_requests {
                sched.enqueue(request).map_err(|e| {
                    EngineError::InitializationError(format!("Failed to seed request: {:?}", e))
                })?;
            }
        }

        println!("🌱 Seeded {} initial requests" /* count */,);
        Ok(())
    }

    fn process_response_with_spiders(
        &self,
        spiders: &[Box<dyn Spider>],
        response: Response,
    ) -> Vec<SpiderResult> {
        // Implementation to route response to correct spider and get results
        vec![]
    }

    async fn perform_request(
        &self,
        client: &reqwest::Client,
        request: Request,
    ) -> Result<Response, reqwest::Error> {
        // Implement actual HTTP request logic here
        // This is a placeholder
        Ok(Response::new())
    }
}
