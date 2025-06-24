use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Mutex,
};

use tracing::warn;

use crate::spider::ResultItem;

pub trait Pipeline: Send + Sync {
    fn type_id(&self) -> TypeId;
    fn try_process(&mut self, item: Option<Box<dyn Any + Send>>) -> Option<Box<dyn Any + Send>>;
    fn close(&mut self);
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

    fn try_process(&mut self, item: Option<Box<dyn Any + Send>>) -> Option<Box<dyn Any + Send>> {
        let typed: Option<T> = item.and_then(|b| b.downcast::<T>().ok().map(|b| *b));
        let result = (self.handler)(typed);
        result.map(|r| Box::new(r) as Box<dyn Any + Send>)
    }

    fn close(&mut self) {}
}

struct PrioritizedPipeline {
    pub priority: usize,
    pub pipeline: Box<dyn Pipeline>,
}

#[derive(Clone)]
pub struct PipelineManager {
    pipelines: HashMap<TypeId, Vec<PrioritizedPipeline>>,
}

impl PipelineManager {
    pub fn new() -> Self {
        Self {
            pipelines: HashMap::new(),
        }
    }

    pub fn add_pipeline<T>(&mut self, pipeline: impl Pipeline + 'static, priority: usize)
    where
        T: 'static + Send + Sync,
    {
        let type_id = TypeId::of::<T>();
        let prioritized = PrioritizedPipeline {
            priority,
            pipeline: Box::new(pipeline),
        };

        self.pipelines
            .entry(type_id)
            .or_insert_with(|| Mutex::new(Vec::new()))
            .lock()
            .unwrap()
            .push(prioritized);

        if let Some(mutex) = self.pipelines.get(&type_id) {
            let mut vec = mutex.lock().unwrap();
            vec.sort_by_key(|pp| pp.priority);
        }
    }

    pub fn process_item(&self, item: ResultItem) {
        let type_id = item.as_ref().type_id();
        let mut current = Some(item);

        if let Some(mutex) = self.pipelines.get(&type_id) {
            let mut pipelines = mutex.lock().unwrap();
            for pp in pipelines.iter_mut() {
                current = pp.pipeline.try_process(current);
            }
        } else {
            warn!("No pipeline for type {:?}", type_id);
        }
    }

    pub fn close_all(&self) {
        for (_, pipeline_group) in &self.pipelines {
            let mut pipelines = pipeline_group.lock().unwrap();
            for pp in pipelines.iter_mut() {
                pp.pipeline.close();
            }
        }
    }
}
