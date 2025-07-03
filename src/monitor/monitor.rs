use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tracing::info;

use crate::{
    config::EngineConfig, downloader::downloader::Downloader, monitor::tui::TuiMonitor,
    pipeline::manager::PipelineManager, scheduler::scheduler::Scheduler,
    spider::manager::SpiderManager,
};

pub struct EngineMonitor {
    pub downloader: Arc<Downloader>,
    pub spider_manager: Arc<SpiderManager>,
    pub scheduler: Arc<dyn Scheduler>,
    pub pipeline_manager: Arc<PipelineManager>,

    pub shutdown_signal: Arc<AtomicBool>,
    pub last_activity: Arc<Mutex<Instant>>,
    pub config: EngineConfig,
}

impl EngineMonitor {
    pub fn new(
        downloader: Arc<Downloader>,
        spider_manager: Arc<SpiderManager>,
        scheduler: Arc<dyn Scheduler>,
        pipeline_manager: Arc<PipelineManager>,

        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<Mutex<Instant>>,
        config: EngineConfig,
    ) -> Self {
        Self {
            downloader,
            scheduler,
            pipeline_manager,
            spider_manager,

            shutdown_signal,
            last_activity,
            config,
        }
    }

    pub fn start(&self) {
        loop {
            std::thread::sleep(Duration::from_secs(1));

            let active = self.downloader.get_stats().active_requests;
            let idle_time = self.last_activity.lock().unwrap().elapsed();
            let scheduler_empty = self.scheduler.is_empty();

            let spider_manager_stats = self.spider_manager.get_stats();
            let pipeline_manager_stats = self.pipeline_manager.get_stats();

            if active == 0
                && scheduler_empty
                && idle_time >= self.config.idle_timeout
                && spider_manager_stats.pending_spiders == 0
                && spider_manager_stats.active_spiders == 0
                && pipeline_manager_stats.processing_items == 0
            {
                info!("⏰ All work completed, initiating shutdown...");
                self.shutdown_signal.store(true, Ordering::Relaxed);
                break;
            }

            if self.shutdown_signal.load(Ordering::Relaxed) {
                break;
            }
        }
        info!("💊 Health check thread stopped");
    }

    pub fn start_tui(&self) -> Result<(), Box<dyn std::error::Error>> {
        let downloader = Arc::clone(&self.downloader);
        let scheduler = Arc::clone(&self.scheduler);
        let shutdown_signal = Arc::clone(&self.shutdown_signal);
        let last_activity = Arc::clone(&self.last_activity);
        let spider_manager = Arc::clone(&self.spider_manager);
        let pipeline_manager = Arc::clone(&self.pipeline_manager);
        let config = self.config.clone();

        let tui_monitor = TuiMonitor::new(
            downloader,
            scheduler,
            spider_manager,
            pipeline_manager,
            shutdown_signal,
            last_activity,
            config,
        );
        tui_monitor.run()
    }
}
