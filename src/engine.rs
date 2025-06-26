use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam::channel::{Receiver, Sender, unbounded};
use governor::{
    RateLimiter,
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
};
use reqwest::Client;
use tokio::{runtime::Runtime, sync::Semaphore, task::JoinSet};
use tracing::{debug, error, info, warn};

use crate::{
    config::{self, EngineConfig},
    downloader::Downloader,
    errors::EngineError,
    pipeline::PipelineManager,
    request::Request,
    response::Response,
    scheduler::{Scheduler, SimpleScheduler},
    spider::{ResultItem, Spider, SpiderResult},
};

pub struct Engine {
    scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
    spiders: Vec<Box<dyn Spider>>,
    pipeline_manager: Arc<PipelineManager>,
    config: EngineConfig,
    downloader: Arc<Downloader>,

    active_requests: Arc<AtomicUsize>,
    shutdown_signal: Arc<AtomicBool>,
    last_activity: Arc<std::sync::Mutex<Instant>>,
}

impl Engine {
    pub fn new(
        scheduler: Box<dyn Scheduler + Send + Sync>,
        spiders: Vec<Box<dyn Spider>>,
        pipelines: PipelineManager,
        config: Option<EngineConfig>,
    ) -> Self {
        let config = config.unwrap_or_default();

        let downloader = match Downloader::new(&config) {
            Ok(d) => Arc::new(d),
            Err(e) => {
                error!("Downloader initialization failed: {}", e);
                panic!();
            }
        };

        Self {
            scheduler: Arc::new(Mutex::new(scheduler)),
            spiders,
            pipeline_manager: Arc::new(pipelines),
            config,
            downloader,
            active_requests: Arc::new(AtomicUsize::new(0)),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            last_activity: Arc::new(std::sync::Mutex::new(Instant::now())),
        }
    }

    pub fn start(&mut self) -> Result<(), EngineError> {
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
            // 0. Seed initial requests
            self.seed_initial_requests_to_scheduler()?;

            // 1. Spawn Downloader Thread
            // Downloader pulls requests directly from scheduler
            let _downloader_handles = {
                let downloader = self.downloader.clone();

                let scheduler = Arc::clone(&self.scheduler);
                let resp_sender = resp_sender.clone();
                let active_requests = Arc::clone(&active_requests);
                let shutdown_signal = Arc::clone(&shutdown_signal);
                let last_activity = Arc::clone(&last_activity);
                let config = self.config.clone();

                scope.spawn(move |_| {
                    downloader.start(
                        scheduler,
                        resp_sender,
                        active_requests,
                        shutdown_signal,
                        last_activity,
                        &config,
                    )
                })
            };

            // 2. Spawn Spider Manager Thread
            // Spider Manager pushes requests to scheduler
            let _spider_handle = {
                let scheduler = Arc::clone(&self.scheduler);
                let resp_receiver = resp_receiver.clone();
                let item_sender = item_sender.clone();
                let active_requests = Arc::clone(&active_requests);
                let shutdown_signal = Arc::clone(&shutdown_signal);
                let last_activity = Arc::clone(&last_activity);
                let spiders = std::mem::take(&mut self.spiders);

                scope.spawn(move |_| {
                    println!("🕷️  Spider Manager thread started");
                    Self::run_spider_manager(
                        spiders,
                        scheduler,
                        resp_receiver,
                        item_sender,
                        active_requests,
                        shutdown_signal,
                        last_activity,
                    )
                })
            };

            // 3. Pipeline Manager threads (unchanged)
            let _pipeline_handles: Vec<_> = (0..self.config.pipeline_worker_threads)
                .map(|i| {
                    let item_receiver = item_receiver.clone();
                    let shutdown_signal = Arc::clone(&shutdown_signal);
                    let pipeline_manager = Arc::clone(&self.pipeline_manager);

                    scope.spawn(move |_| {
                        println!("🔧 Pipeline Manager thread {} started", i);
                        Self::run_pipeline_manager(
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
                let scheduler = Arc::clone(&self.scheduler);

                scope.spawn(move |_| {
                    println!("💊 Health check thread started");
                    Self::run_health_check(
                        active_requests,
                        scheduler,
                        shutdown_signal,
                        last_activity,
                        config,
                    )
                })
            };

            println!("🚀 All threads spawned, engine running...");

            // Wait for completion
            let _ = health_handle.join();
            println!("🛑 Shutdown signal received, waiting for threads to finish...");

            Ok(())
        })
        .unwrap()
    }

    // CORRECTED: Spider Manager pushes to scheduler
    fn run_spider_manager(
        spiders: Vec<Box<dyn Spider>>,
        scheduler: Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
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

                    let spider_result = response.request.spider.parse(response.clone());
                    match spider_result {
                        SpiderResult::Requests(requests) => {
                            // Push new requests to scheduler
                            let mut sched = scheduler.lock().unwrap();
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
                            let mut sched = scheduler.lock().unwrap();
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
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }

        println!("🕷️  Spider Manager thread stopped");
    }

    fn run_pipeline_manager(
        thread_id: usize,
        pipeline_manager: Arc<PipelineManager>,
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
        active_requests: Arc<AtomicUsize>,
        scheduler: Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<std::sync::Mutex<Instant>>,
        config: EngineConfig,
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

    fn seed_initial_requests_to_scheduler(&self) -> Result<(), EngineError> {
        println!("🌱 Seeding initial requests to scheduler...");

        let mut sched = self.scheduler.lock().unwrap();
        let mut start_requests_count = 0;

        println!("Heyyyyyyyyyyyyyyyyyy, {}", &self.spiders.iter().count());

        // Get start requests from each spider
        for spider in &self.spiders {
            let start_requests = spider.start_requests();

            for request in start_requests {
                sched.enqueue(request).map_err(|e| {
                    EngineError::InitializationError(format!("Failed to seed request: {:?}", e))
                })?;
                start_requests_count += 1;
            }
        }

        println!("🌱 Seeded {} initial requests", start_requests_count);
        Ok(())
    }

    async fn perform_request(
        config: &EngineConfig,
        client: &reqwest::Client,
        request: Request,
        download_sem: Arc<Semaphore>,
    ) -> Option<Response> {
        let _permit = download_sem.acquire().await.unwrap();

        // Implement actual HTTP request logic here
        // This is a placeholder
        let req_clone = request.clone();
        let resp = client
            .request(request.method, request.url)
            .headers(request.headers.unwrap_or_default())
            .body(request.body.unwrap_or_default())
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                let url = r.url().clone();

                if r.error_for_status_ref().is_ok()
                    || config.http_error_allow_codes.contains(&status)
                {
                    let body = r.text().await.unwrap_or_default();
                    Some(Response {
                        status,
                        body,
                        request: req_clone,
                    })
                } else {
                    error!("Status error [{}]: {}", status, url);
                    None
                }
            }
            Err(e) => {
                if e.is_timeout() {
                    error!("Timeout: {} -> {}", req_clone.url, e);
                } else if e.is_connect() {
                    error!(
                        "Connection error (maybe server not started): {} -> {}",
                        req_clone.url, e
                    );
                } else if e.is_request() {
                    error!("Bad request formation: {} -> {}", req_clone.url, e);
                } else if e.is_body() {
                    error!("Body error: {} -> {}", req_clone.url, e);
                } else {
                    error!("Request failed (other): {} -> {:?}", req_clone.url, e);
                }

                None
            }
        }
    }
}
