use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{config::EngineConfig, scheduler::Scheduler};

pub struct EngineMonitor {
    active_requests: Arc<AtomicUsize>,
    scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
    shutdown_signal: Arc<AtomicBool>,
    last_activity: Arc<Mutex<Instant>>,
    config: EngineConfig,
}

impl EngineMonitor {
    pub fn new(
        active_requests: Arc<AtomicUsize>,
        scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<Mutex<Instant>>,
        config: EngineConfig,
    ) -> Self {
        Self {
            active_requests,
            scheduler,
            shutdown_signal,
            last_activity,
            config,
        }
    }

    pub fn start(self) {
        let mut stats_timer = Instant::now();

        loop {
            std::thread::sleep(Duration::from_secs(1));

            if stats_timer.elapsed() >= self.config.stats_interval {
                let active = self.active_requests.load(Ordering::Relaxed);
                let scheduler_empty = self.scheduler.lock().unwrap().is_empty();

                println!(
                    "📊 Active requests: {}, Scheduler empty: {}",
                    active, scheduler_empty
                );

                stats_timer = Instant::now();
            }

            let idle_time = self.last_activity.lock().unwrap().elapsed();
            let active = self.active_requests.load(Ordering::Relaxed);
            let scheduler_empty = self.scheduler.lock().unwrap().is_empty();

            if active == 0 && scheduler_empty && idle_time >= self.config.idle_timeout {
                println!("⏰ All work completed, initiating shutdown...");
                self.shutdown_signal.store(true, Ordering::Relaxed);
                break;
            }

            if self.shutdown_signal.load(Ordering::Relaxed) {
                break;
            }
        }

        println!("💊 Health check thread stopped");
    }
}
