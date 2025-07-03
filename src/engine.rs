use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use crossbeam::channel::unbounded;
use tracing::{debug, error, info};

use crate::{
    config::EngineConfig,
    downloader::downloader::Downloader,
    errors::EngineError,
    item::ResultItem,
    monitor::monitor::EngineMonitor,
    pipeline::manager::PipelineManager,
    response::Response,
    scheduler::scheduler::Scheduler,
    spider::{manager::SpiderManager, spider::Spider},
    utils::human_duration,
};

pub struct Engine {
    spider_manager: Arc<SpiderManager>,
    pipeline_manager: Arc<PipelineManager>,
    downloader: Arc<Downloader>,
    scheduler: Arc<dyn Scheduler>,
    monitor: Arc<EngineMonitor>,
    config: EngineConfig,

    shutdown_signal: Arc<AtomicBool>,
    last_activity: Arc<Mutex<Instant>>,
    start_time: Instant,
}

impl Engine {
    pub fn new(
        scheduler: Arc<dyn Scheduler>,
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
        let spider_manager = Arc::new(SpiderManager::new(spiders, &config));
        let pipeline_manager = Arc::new(pipelines);

        let shutdown_signal = Arc::new(AtomicBool::new(false));
        let last_activity = Arc::new(Mutex::new(Instant::now()));

        let monitor = Arc::new(EngineMonitor::new(
            Arc::clone(&downloader),
            Arc::clone(&spider_manager),
            Arc::clone(&scheduler),
            Arc::clone(&pipeline_manager),
            Arc::clone(&shutdown_signal),
            Arc::clone(&last_activity),
            config.clone(),
        ));
        let start_time = Instant::now();

        Self {
            scheduler,
            spider_manager,
            pipeline_manager,
            downloader,
            shutdown_signal,
            last_activity,
            monitor,
            start_time,
            config,
        }
    }

    pub fn start(&mut self) -> Result<(), EngineError> {
        self.start_time = Instant::now();

        let spider_stats = self.spider_manager.get_stats();
        info!(
            "🕷️ Starting Iron-Spider Engine {} spiders",
            spider_stats.total_spiders
        );

        // Create communication channels
        let (resp_sender, resp_receiver) = unbounded::<Response>();
        let (item_sender, item_receiver) = unbounded::<ResultItem>();

        let show_tui = self.config.show_tui;

        // Use crossbeam::scope for structured concurrency
        crossbeam::scope(|scope| {
            let mut handles = vec![];

            // 1. Spawn Spider Manager thread
            // Spider Manager pushes requests to scheduler
            handles.push(scope.spawn({
                let scheduler = Arc::clone(&self.scheduler);
                let resp_receiver = resp_receiver.clone();
                let item_sender = item_sender.clone();
                let shutdown_signal = Arc::clone(&self.shutdown_signal);
                let err_handler_shutdown_signal = Arc::clone(&shutdown_signal);
                let last_activity = Arc::clone(&self.last_activity);

                let spider_manager = self.spider_manager.clone();

                move |_| {
                    debug!(" Spider Manager thread is starting");
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
                }
            }));

            // 2. Spawn Downloader thread
            // Downloader pulls requests directly from scheduler
            handles.push(scope.spawn({
                let downloader = self.downloader.clone();

                let scheduler = Arc::clone(&self.scheduler);
                let resp_sender = resp_sender.clone();
                let shutdown_signal = Arc::clone(&self.shutdown_signal);
                let last_activity = Arc::clone(&self.last_activity);

                debug!("Downloader thread is starting");
                move |_| downloader.start(scheduler, resp_sender, shutdown_signal, last_activity)
            }));

            // 3. Pipeline Manager thread
            handles.push(scope.spawn({
                let item_receiver = item_receiver.clone();
                let shutdown_signal = Arc::clone(&self.shutdown_signal);
                let pipeline_manager = Arc::clone(&self.pipeline_manager);

                debug!("Pipeline Manager thread is starting");
                move |_| pipeline_manager.start(item_receiver, shutdown_signal)
            }));

            // 5. Spawn Health Check & Stats Thread
            handles.push(scope.spawn({
                let monitor = Arc::clone(&self.monitor);
                move |_| {
                    debug!("Health-check thread is starting");
                    let _ = monitor.start();
                }
            }));

            // 6. Spawn Health Check & Stats Thread TUI
            if show_tui {
                handles.push(scope.spawn({
                    let monitor = Arc::clone(&self.monitor);

                    move |_| {
                        debug!("TUI thread is starting");
                        let _ = monitor.start_tui();
                    }
                }))
            };

            info!("🚀 All threads spawned, engine running...");

            // Wait for all threads
            for h in handles {
                let _ = h.join();
            }

            info!("🛑 Shutdown signal received, waiting for threads to finish...");
            self.defer();

            Ok(())
        })
        .unwrap()
    }

    fn defer(&self) {
        let exec_duration = self.start_time.elapsed();

        let downloader_stats = self.downloader.get_stats();
        let spider_manager_stats = self.spider_manager.get_stats();
        let scheduler_stats = self.scheduler.get_stats();
        let pipeline_manager_stats = self.pipeline_manager.get_stats();

        println!("{:-^50}", "Spider Manager Stats");
        println!("{}", spider_manager_stats);

        println!("{:-^50}", "Scheduler Stats");
        println!("{}", scheduler_stats);

        println!("{:-^50}", "Downloader Stats");
        println!("{}", downloader_stats);

        println!("{:-^50}", "Pipeline Manager Stats");
        println!("{}", pipeline_manager_stats);

        println!("{:-^50}", "Execution Duration");
        println!("{:^50}", human_duration(exec_duration));
    }
}
