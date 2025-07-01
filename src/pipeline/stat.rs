use std::{
    fmt::{self, Display},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::utils::format_number;

pub struct PipelineManagerStats {
    // Number of items on processing pool.
    pub processing_items: usize,

    // Number of items processed by pipelines.
    pub processed_items: usize,

    // If any pipeline return a None,
    // which mean the item push in is dropped by the pipeline.
    // Number of items dropped by the pipelines.
    pub dropped_items: usize,

    // No pipeline found for these items.
    pub unrouted_item: usize,
}

impl Display for PipelineManagerStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Processing: {}",
            format_number(self.processing_items as u64)
        )?;
        writeln!(
            f,
            "Processed : {}",
            format_number(self.processed_items as u64)
        )?;
        writeln!(
            f,
            "Dropped   : {}",
            format_number(self.dropped_items as u64)
        )?;
        writeln!(
            f,
            "Un-routed : {}",
            format_number(self.unrouted_item as u64)
        )?;
        Ok(())
    }
}

pub(crate) struct PipelineManagerStatsTracker {
    // Number of items on processing pool.
    processing_items: Arc<AtomicUsize>,

    // Number of items processed by pipelines.
    processed_items: Arc<AtomicUsize>,

    // If any pipeline return a None,
    // which mean the item push in is dropped by the pipeline.
    // Number of items dropped by the pipelines.
    dropped_items: Arc<AtomicUsize>,

    // No pipeline found for these items.
    unrouted_item: Arc<AtomicUsize>,
}

impl PipelineManagerStatsTracker {
    pub fn new() -> Self {
        Self {
            processing_items: Arc::new(AtomicUsize::new(0)),
            processed_items: Arc::new(AtomicUsize::new(0)),
            dropped_items: Arc::new(AtomicUsize::new(0)),
            unrouted_item: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn get_stats(&self) -> PipelineManagerStats {
        PipelineManagerStats {
            processing_items: self.processing_items.load(Ordering::Relaxed),
            processed_items: self.processed_items.load(Ordering::Relaxed),
            dropped_items: self.dropped_items.load(Ordering::Relaxed),
            unrouted_item: self.unrouted_item.load(Ordering::Relaxed),
        }
    }

    pub fn start_one(&self) {
        self.processing_items.fetch_add(1, Ordering::Relaxed);
    }

    pub fn processed_one(&self) {
        self.processing_items.fetch_sub(1, Ordering::Relaxed);
        self.processed_items.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dropped_one(&self) {
        self.processing_items.fetch_sub(1, Ordering::Relaxed);
        self.dropped_items.fetch_add(1, Ordering::Relaxed);
    }

    pub fn unrouted_one(&self) {
        self.unrouted_item.fetch_add(1, Ordering::Relaxed);
    }

    pub fn is_idle(&self) -> bool {
        self.processing_items.load(Ordering::Relaxed) == 0
    }
}
