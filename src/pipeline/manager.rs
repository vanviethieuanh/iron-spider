use std::{
    any::TypeId,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crossbeam::channel::Receiver;
use dashmap::DashMap;
use rayon::{
    ThreadPool, ThreadPoolBuilder,
    iter::{IntoParallelIterator, ParallelIterator},
};
use tracing::{info, warn};

use crate::{
    config::EngineConfig,
    item::ResultItem,
    pipeline::{
        pipeline::Pipeline,
        stat::{PipelineManagerStats, PipelineManagerStatsTracker},
    },
};

#[derive(Clone)]
struct PrioritizedPipeline {
    pub priority: usize,
    pub pipeline: Arc<dyn Pipeline>,
}

pub struct PipelineManager {
    pipelines: Arc<DashMap<TypeId, Vec<PrioritizedPipeline>>>,
    thread_pool: ThreadPool,
    stats_tracker: Arc<PipelineManagerStatsTracker>,
}

impl PipelineManager {
    pub fn new(config: &EngineConfig) -> Self {
        Self {
            pipelines: Arc::new(DashMap::new()),
            thread_pool: ThreadPoolBuilder::new()
                .num_threads(config.pipeline_worker_threads)
                .build()
                .unwrap(),
            stats_tracker: Arc::new(PipelineManagerStatsTracker::new()),
        }
    }

    pub fn add_pipeline<T>(&mut self, pipeline: impl Pipeline + 'static, priority: usize)
    where
        T: 'static + Send + Sync,
    {
        let type_id = TypeId::of::<T>();
        let prioritized = PrioritizedPipeline {
            priority,
            pipeline: Arc::new(pipeline),
        };

        self.pipelines
            .entry(type_id)
            .and_modify(|vec| {
                vec.push(prioritized.clone());
                vec.sort_by_key(|pp| pp.priority);
            })
            .or_insert_with(|| vec![prioritized]);
    }

    pub fn process_item(&self, item: ResultItem) {
        let type_id = item.type_id();
        let pipelines = self.pipelines.get(&type_id).map(|entry| entry.clone());

        let stats_tracker = self.stats_tracker.clone();

        if let Some(pipelines) = pipelines {
            stats_tracker.start_one();
            self.thread_pool.spawn(move || {
                let mut current = Some(item);

                // Process through all pipelines in priority order
                for pp in pipelines.iter() {
                    if current.is_none() {
                        break; // Item was dropped by a previous pipeline
                    }
                    current = pp.pipeline.try_process(current);
                }

                if current.is_some() {
                    stats_tracker.processed_one();
                } else {
                    stats_tracker.dropped_one();
                }
            });
        } else {
            warn!("No pipeline for type {:?}", type_id);
            stats_tracker.unrouted_one();
        }
    }

    pub fn get_stats(&self) -> PipelineManagerStats {
        self.stats_tracker.get_stats()
    }

    pub fn close_all_pipelines(&self) {
        let all_pipelines: Vec<Arc<dyn Pipeline>> = self
            .pipelines
            .iter()
            .flat_map(|entry| {
                // Clone the whole vector here so we don't return a reference
                entry
                    .value()
                    .iter()
                    .map(|pp| pp.pipeline.clone())
                    .collect::<Vec<_>>()
            })
            .collect();

        all_pipelines.into_par_iter().for_each(|pipeline| {
            pipeline.close();
        });
    }

    pub fn wait_for_completion(&self) {
        loop {
            if self.stats_tracker.is_idle() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn start(&self, item_receiver: Receiver<ResultItem>, shutdown_signal: Arc<AtomicBool>) {
        while !shutdown_signal.load(Ordering::Relaxed) || !item_receiver.is_empty() {
            match item_receiver.try_recv() {
                Ok(item) => {
                    // Process item through pipeline
                    self.process_item(item);
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }

        self.wait_for_completion();
        self.close_all_pipelines();

        info!("🔧 Pipeline Manager thread stopped");
    }
}
