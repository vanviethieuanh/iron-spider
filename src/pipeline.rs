use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use tracing::warn;

pub trait Pipeline: Send + Sync {
    fn type_id(&self) -> TypeId;
    fn try_process(&self, item: Option<Box<dyn Any>>) -> Option<Box<dyn Any>>;
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

    fn try_process(&self, item: Option<Box<dyn Any>>) -> Option<Box<dyn Any>> {
        let typed: Option<T> = item.and_then(|b| b.downcast::<T>().ok().map(|b| *b));
        let result = (self.handler)(typed);
        result.map(|r| Box::new(r) as Box<dyn Any>)
    }
}

struct PrioritizedPipeline {
    pub priority: usize,
    pub pipeline: Box<dyn Pipeline>,
}

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
        let entry = self.pipelines.entry(type_id).or_default();
        entry.push(PrioritizedPipeline {
            priority,
            pipeline: Box::new(pipeline),
        });
        entry.sort_by_key(|pp| pp.priority);
    }

    pub fn process_item(&self, item: Box<dyn Any>) {
        let type_id = item.as_ref().type_id();
        let mut current: Option<Box<dyn Any>> = Some(item);

        if let Some(pipes) = self.pipelines.get(&type_id) {
            for pp in pipes {
                current = pp.pipeline.try_process(current);
            }
        } else {
            warn!("No pipeline for type {:?}", type_id);
        }
    }
}
