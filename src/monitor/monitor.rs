use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tracing::info;

use crate::{config::EngineConfig, downloader::downloader::Downloader, scheduler::Scheduler};

pub struct EngineMonitor {
    pub downloader: Arc<Downloader>,
    pub scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
    pub shutdown_signal: Arc<AtomicBool>,
    pub last_activity: Arc<Mutex<Instant>>,
    pub config: EngineConfig,
}

impl EngineMonitor {
    pub fn new(
        downloader: Arc<Downloader>,
        scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<Mutex<Instant>>,
        config: EngineConfig,
    ) -> Self {
        Self {
            downloader,
            scheduler,
            shutdown_signal,
            last_activity,
            config,
        }
    }

    pub fn start(self) {
        loop {
            std::thread::sleep(Duration::from_secs(1));

            let active = self.downloader.get_stats().active_requests;
            let idle_time = self.last_activity.lock().unwrap().elapsed();
            let scheduler_empty = self.scheduler.lock().unwrap().is_empty();

            if active == 0 && scheduler_empty && idle_time >= self.config.idle_timeout {
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
}
