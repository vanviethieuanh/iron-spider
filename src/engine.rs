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
    tasks: JoinSet<()>,

    active_requests: Arc<AtomicUsize>,
    shutdown_signal: Arc<AtomicBool>,
    last_activity: Arc<std::sync::Mutex<Instant>>,
    limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
    max_concurrent_download: Arc<Semaphore>,
}

impl Engine {
    pub fn new(
        scheduler: Box<dyn Scheduler + Send + Sync>,
        spiders: Vec<Box<dyn Spider>>,
        pipelines: PipelineManager,
        config: Option<EngineConfig>,
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

        let downloader_limiter = match config.downloader_request_quota {
            Some(q) => Some(Arc::new(RateLimiter::direct(q))),
            None => None,
        };

        let concurrent_limit = config.concurrent_limit.clone();

        Self {
            scheduler: Arc::new(Mutex::new(scheduler)),
            spiders,
            pipeline_manager: Arc::new(pipelines),
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
            limiter: downloader_limiter,
            max_concurrent_download: Arc::new(Semaphore::new(concurrent_limit)),
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

        let config = self.config.clone();

        // Use crossbeam::scope for structured concurrency
        crossbeam::scope(|scope| {
            // 0. Seed initial requests
            self.seed_initial_requests_to_scheduler()?;

            // 1. Spawn Downloader Threads
            // Downloader pulls requests directly from scheduler
            let _downloader_handles: Vec<_> = (0..config.downloader_threads)
                .map(|i| {
                    let scheduler = Arc::clone(&self.scheduler);
                    let resp_sender = resp_sender.clone();
                    let active_requests = Arc::clone(&active_requests);
                    let shutdown_signal = Arc::clone(&shutdown_signal);
                    let last_activity = Arc::clone(&last_activity);
                    let concurrent_limit = Arc::clone(&self.max_concurrent_download);
                    let config = config.clone();

                    scope.spawn(move |_| {
                        println!("⬇️  Downloader thread {} started", i);
                        Self::run_downloader(
                            i as usize,
                            scheduler,
                            resp_sender,
                            active_requests,
                            shutdown_signal,
                            last_activity,
                            &config,
                            concurrent_limit,
                        )
                    })
                })
                .collect();

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

    fn run_downloader(
        thread_id: usize,
        scheduler: Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
        resp_sender: Sender<Response>,
        active_requests: Arc<AtomicUsize>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<std::sync::Mutex<Instant>>,
        config: &EngineConfig,
        download_sem: Arc<Semaphore>,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .timeout(config.downloader_request_timeout)
                .connect_timeout(Duration::from_millis(200))
                .connector_layer(tower::limit::concurrency::ConcurrencyLimitLayer::new(2))
                .build()
                .unwrap();

            let mut join_set = tokio::task::JoinSet::new();

            while !shutdown_signal.load(Ordering::Relaxed) {
                // Pull request from scheduler
                let request = {
                    let mut sched = scheduler.lock().unwrap();
                    sched.dequeue()
                };

                match request {
                    Some(req) => {
                        active_requests.fetch_add(1, Ordering::Relaxed);
                        *last_activity.lock().unwrap() = Instant::now();

                        let client = client.clone();
                        let resp_sender = resp_sender.clone();
                        let config = config.clone();
                        let active_requests = Arc::clone(&active_requests);
                        let download_sem_clone = download_sem.clone();

                        // Perform HTTP request
                        join_set.spawn(async move {
                            if let Some(response) =
                                Self::perform_request(&config, &client, req, download_sem_clone)
                                    .await
                            {
                                let _ = resp_sender.send(response);
                            }
                            active_requests.fetch_sub(1, Ordering::Relaxed);
                        });
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
