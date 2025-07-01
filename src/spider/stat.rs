use std::sync::atomic::{AtomicUsize, Ordering};

use std::fmt;
use tracing::debug;

pub struct SpiderManagerStats {
    pub dropped_responses: usize,
    pub total_spiders: usize,
    pub pending_spiders: usize,
    pub closed_spiders: usize,
    pub active_spiders: usize,
}

impl fmt::Display for SpiderManagerStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total = self.total_spiders.max(1) as f64;

        writeln!(f, "===== Spider Manager Stats =====")?;
        writeln!(f, "Total Spiders       : {:>5}", self.total_spiders)?;
        writeln!(
            f,
            "Pending             : {:>5} ({:>5.2}%)",
            self.pending_spiders,
            self.pending_spiders as f64 / total * 100.0
        )?;
        writeln!(
            f,
            "Active              : {:>5} ({:>5.2}%)",
            self.active_spiders,
            self.active_spiders as f64 / total * 100.0
        )?;
        writeln!(
            f,
            "Closed              : {:>5} ({:>5.2}%)",
            self.closed_spiders,
            self.closed_spiders as f64 / total * 100.0
        )?;
        writeln!(f, "Dropped Responses   : {:>5}", self.dropped_responses)
    }
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

    pub(super) fn get_stats(&self, pending_spider_count: usize) -> SpiderManagerStats {
        let active = self.active_spiders.load(Ordering::Relaxed);
        let dropped = self.dropped_responses.load(Ordering::Relaxed);

        SpiderManagerStats {
            pending_spiders: pending_spider_count,
            dropped_responses: dropped,
            total_spiders: self.spider_counts,
            active_spiders: active,
            closed_spiders: self.spider_counts - pending_spider_count - active,
        }
    }
}
