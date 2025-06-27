use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam::channel::unbounded;
use tracing::{error, info};

use crate::{
    config::EngineConfig,
    downloader::Downloader,
    errors::EngineError,
    item::ResultItem,
    pipeline::manager::PipelineManager,
    response::Response,
    scheduler::Scheduler,
    spider::{manager::SpiderManager, spider::Spider},
};

pub struct Engine {
    scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
    spider_manager: Arc<SpiderManager>,
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
        spiders: Vec<Arc<dyn Spider>>,
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

        let spider_manager = Arc::new(SpiderManager::new(spiders));

        Self {
            scheduler: Arc::new(Mutex::new(scheduler)),
            spider_manager,
            pipeline_manager: Arc::new(pipelines),
            config,
            downloader,
            active_requests: Arc::new(AtomicUsize::new(0)),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            last_activity: Arc::new(std::sync::Mutex::new(Instant::now())),
        }
    }

    pub fn start(&mut self) -> Result<(), EngineError> {
        let spider_stats = self.spider_manager.get_stats();
        info!(
            "🕷️ Starting Iron-Spider Engine {} spiders",
            spider_stats.total_spiders
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
            // 1. Spawn Spider Manager Thread
            // Spider Manager pushes requests to scheduler
            let _spider_handle = {
                let scheduler = Arc::clone(&self.scheduler);
                let resp_receiver = resp_receiver.clone();
                let item_sender = item_sender.clone();
                let shutdown_signal = Arc::clone(&shutdown_signal);
                let err_handler_shutdown_signal = Arc::clone(&shutdown_signal);
                let last_activity = Arc::clone(&last_activity);

                let spider_manager = self.spider_manager.clone();

                scope.spawn(move |_| {
                    if let Err(err) = spider_manager.start(
                        scheduler,
                        resp_receiver,
                        item_sender,
                        shutdown_signal,
                        last_activity,
                    ) {
                        error!("❌ SpiderManager crashed: {}", err);
                        err_handler_shutdown_signal.store(true, Ordering::SeqCst);
                    }
                    println!("🕷️  Spider Manager thread started");
                })
            };

            // 2. Spawn Downloader Thread
            // Downloader pulls requests directly from scheduler
            let _downloader_handles = {
                let downloader = self.downloader.clone();

                let scheduler = Arc::clone(&self.scheduler);
                let resp_sender = resp_sender.clone();
                let active_requests = Arc::clone(&active_requests);
                let shutdown_signal = Arc::clone(&shutdown_signal);
                let last_activity = Arc::clone(&last_activity);

                scope.spawn(move |_| {
                    downloader.start(
                        scheduler,
                        resp_sender,
                        active_requests,
                        shutdown_signal,
                        last_activity,
                    )
                })
            };

            // 3. Pipeline Manager threads
            let _pipeline_handles = {
                let item_receiver = item_receiver.clone();
                let shutdown_signal = Arc::clone(&shutdown_signal);
                let pipeline_manager = Arc::clone(&self.pipeline_manager);

                scope.spawn(move |_| pipeline_manager.start(item_receiver, shutdown_signal))
            };

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
}
