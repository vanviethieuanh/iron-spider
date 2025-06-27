use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};

use crossbeam::channel::unbounded;
use tracing::{error, info};

use crate::{
    config::EngineConfig,
    downloader::Downloader,
    errors::EngineError,
    item::ResultItem,
    monitor::EngineMonitor,
    pipeline::manager::PipelineManager,
    response::Response,
    scheduler::Scheduler,
    spider::{manager::SpiderManager, spider::Spider},
};

pub struct Engine {
    scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
    spider_manager: Arc<SpiderManager>,
    pipeline_manager: Arc<PipelineManager>,
    downloader: Arc<Downloader>,

    shutdown_signal: Arc<AtomicBool>,
    last_activity: Arc<Mutex<Instant>>,

    config: EngineConfig
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
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
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

        // Use crossbeam::scope for structured concurrency
        crossbeam::scope(|scope| {
            // 1. Spawn Spider Manager Thread
            // Spider Manager pushes requests to scheduler
            let _spider_handle = {
                let scheduler = Arc::clone(&self.scheduler);
                let resp_receiver = resp_receiver.clone();
                let item_sender = item_sender.clone();
                let shutdown_signal = Arc::clone(&self.shutdown_signal);
                let err_handler_shutdown_signal = Arc::clone(&shutdown_signal);
                let last_activity = Arc::clone(&self.last_activity);

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
                let shutdown_signal = Arc::clone(&self.shutdown_signal);
                let last_activity = Arc::clone(&self.last_activity);

                scope.spawn(move |_| {
                    downloader.start(scheduler, resp_sender, shutdown_signal, last_activity)
                })
            };

            // 3. Pipeline Manager threads
            let _pipeline_handles = {
                let item_receiver = item_receiver.clone();
                let shutdown_signal = Arc::clone(&self.shutdown_signal);
                let pipeline_manager = Arc::clone(&self.pipeline_manager);

                scope.spawn(move |_| pipeline_manager.start(item_receiver, shutdown_signal))
            };

            // 5. Spawn Health Check & Stats Thread
            let health_handle = {
                let health_check = EngineMonitor::new(
                    Arc::clone(&self.downloader),
                    Arc::clone(&self.scheduler),
                    Arc::clone(&self.shutdown_signal),
                    Arc::clone(&self.last_activity),
                    self.config.clone(),
                );

                scope.spawn(move |_| {
                    println!("💊 Health check thread started");
                    health_check.start();
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
}
