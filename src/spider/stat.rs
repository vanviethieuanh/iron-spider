use std::sync::atomic::{AtomicUsize, Ordering};

use tracing::debug;

pub struct SpiderManagerStats {
    pub dropped_responses: usize,
    pub total_spiders: usize,
    pub sleeping_spiders: usize,
    pub active_spiders: usize,
}

pub(crate) struct SpiderManagerStatsTracker {
    spider_counts: usize,
    active_spiders: AtomicUsize,
    dropped_responses: AtomicUsize,
}

impl SpiderManagerStatsTracker {
    pub(super) fn new(spider_counts: usize) -> Self {
        Self {
            spider_counts,
            active_spiders: AtomicUsize::new(0),
            dropped_responses: AtomicUsize::new(0),
        }
    }

    pub(super) fn activate_one_spider(&self) {
        debug!(
            "activating 1 spider, active spiders: {}",
            self.active_spiders.load(Ordering::Relaxed)
        );
        self.active_spiders.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn deactivate_one_spider(&self) {
        debug!(
            "deactivating 1 spider, active spiders: {}",
            self.active_spiders.load(Ordering::Relaxed)
        );
        self.active_spiders.fetch_sub(1, Ordering::Relaxed);
    }

    pub(super) fn drop_one_response(&self) {
        debug!("Dropping 1 spider");
        self.dropped_responses.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn get_stats(&self) -> SpiderManagerStats {
        let active = self.active_spiders.load(Ordering::Relaxed);
        let dropped = self.dropped_responses.load(Ordering::Relaxed);

        SpiderManagerStats {
            dropped_responses: dropped,
            total_spiders: self.spider_counts,
            active_spiders: active,
            sleeping_spiders: self.spider_counts - active,
        }
    }
}
