use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use rayon::{ThreadPool, ThreadPoolBuilder};
use tracing::warn;

use crate::{config::EngineConfig, spider::ResultItem};

pub trait Pipeline: Send + Sync {
    fn type_id(&self) -> TypeId;
    fn try_process(&self, item: Option<ResultItem>) -> Option<ResultItem>;
    fn close(&self);
}
pub struct FnPipeline<T>
where
    T: 'static + Send + Sync,
{
    handler: Box<dyn Fn(Option<T>) -> Option<T> + Send + Sync>,
}

impl<T> FnPipeline<T>
where
    T: 'static + Send + Sync,
{
    pub fn new(handler: impl Fn(Option<T>) -> Option<T> + Send + Sync + 'static) -> Self {
        Self {
            handler: Box::new(handler),
        }
    }
}

impl<T> Pipeline for FnPipeline<T>
where
    T: 'static + Send + Sync,
{
    fn type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn try_process(&self, item: Option<ResultItem>) -> Option<ResultItem> {
        let typed_item: Option<T> =
            item.and_then(|boxed_any| boxed_any.downcast::<T>().ok().map(|boxed_t| *boxed_t));

        let result = (self.handler)(typed_item);

        result.map(|r| Box::new(r) as Box<dyn Any + Send + Sync>)
    }

    fn close(&self) {}
}

#[derive(Clone)]
struct PrioritizedPipeline {
    pub priority: usize,
    pub pipeline: Arc<dyn Pipeline>,
}

pub struct PipelineManager {
    pipelines: Arc<Mutex<HashMap<TypeId, Vec<PrioritizedPipeline>>>>,
    thread_pool: ThreadPool,
    // Number of items on processing pool.
    processing_items: Arc<AtomicUsize>,
    // Number of items processed by pipelines.
    processed_items: Arc<AtomicUsize>,
    // If any pipeline return a None,
    // which mean the item push in is dropped by the pipeline.
    // Number of items dropped by the pipelines.
    dropped_items: Arc<AtomicUsize>,
}

impl PipelineManager {
    pub fn new(config: &EngineConfig) -> Self {
        Self {
            pipelines: Arc::new(Mutex::new(HashMap::new())),
            thread_pool: ThreadPoolBuilder::new()
                .num_threads(config.pipeline_worker_threads)
                .build()
                .unwrap(),
            processing_items: Arc::new(AtomicUsize::new(0)),
            processed_items: Arc::new(AtomicUsize::new(0)),
            dropped_items: Arc::new(AtomicUsize::new(0)),
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

        if let Some(pipelines) = pipelines {
            // Clone the atomic counters for the spawned thread
            let processing_items = Arc::clone(&self.processing_items);
            let processed_items = Arc::clone(&self.processed_items);
            let dropped_items = Arc::clone(&self.dropped_items);

            // Increment processing counter
            processing_items.fetch_add(1, Ordering::Relaxed);

            self.thread_pool.spawn(move || {
                let mut current = Some(item);

                // Process through all pipelines in priority order
                for pp in pipelines.iter() {
                    if current.is_none() {
                        break; // Item was dropped by a previous pipeline
                    }
                    current = pp.pipeline.try_process(current);
                }

                // Update statistics
                processing_items.fetch_sub(1, Ordering::Relaxed);

                if current.is_some() {
                    processed_items.fetch_add(1, Ordering::Relaxed);
                } else {
                    dropped_items.fetch_add(1, Ordering::Relaxed);
                }
            });
        } else {
            warn!("No pipeline for type {:?}", type_id);
            // Still count this as processed since we handled it
            self.processed_items.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn get_stats(&self) -> (usize, usize, usize) {
        (
            self.processing_items.load(Ordering::Relaxed),
            self.processed_items.load(Ordering::Relaxed),
            self.dropped_items.load(Ordering::Relaxed),
        )
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
}
