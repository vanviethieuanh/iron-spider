use std::{
    any::TypeId,
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crossbeam::channel::Receiver;
use rayon::{ThreadPool, ThreadPoolBuilder};
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
    pipelines: Arc<Mutex<HashMap<TypeId, Vec<PrioritizedPipeline>>>>,
    thread_pool: ThreadPool,
    stats_tracker: Arc<PipelineManagerStatsTracker>,
}

impl PipelineManager {
    pub fn new(config: &EngineConfig) -> Self {
        Self {
            pipelines: Arc::new(Mutex::new(HashMap::new())),
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

        if let Ok(mut pipelines) = self.pipelines.lock() {
            pipelines
                .entry(type_id)
                .or_insert_with(Vec::new)
                .push(prioritized);

            if let Some(vec) = pipelines.get_mut(&type_id) {
                vec.sort_by_key(|pp| pp.priority);
            }
        }
    }

    pub fn process_item(&self, item: ResultItem) {
        let type_id = item.type_id();

        // Get pipelines for this type
        let pipelines = if let Ok(pipeline_map) = self.pipelines.lock() {
            pipeline_map.get(&type_id).cloned()
        } else {
            warn!("Failed to acquire lock on pipelines");
            return;
        };

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
        if let Ok(pipelines) = self.pipelines.lock() {
            for (_, pipeline_vec) in pipelines.iter() {
                for pp in pipeline_vec.iter() {
                    pp.pipeline.close();
                }
            }
        }
    }

    pub fn start(&self, item_receiver: Receiver<ResultItem>, shutdown_signal: Arc<AtomicBool>) {
        while !shutdown_signal.load(Ordering::Relaxed) {
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

        info!("🔧 Pipeline Manager thread stopped");
    }
}
