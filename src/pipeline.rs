use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use tracing::warn;

pub struct Pipeline {
    pub type_id: TypeId,
    pub process: Box<dyn Fn(Option<Box<dyn Any>>) -> Option<Box<dyn Any>> + Send + Sync>,
}

impl Pipeline {
    pub fn new<T>(handler: impl Fn(Option<T>) -> Option<T> + Send + Sync + 'static) -> Self
    where
        T: 'static + Send + Sync,
    {
        let type_id = TypeId::of::<T>();
        let process = Box::new(move |item: Option<Box<dyn Any>>| {
            let typed: Option<T> = item.and_then(|b| b.downcast::<T>().ok().map(|b| *b));
            let result = handler(typed);
            result.map(|r| Box::new(r) as Box<dyn Any>)
        });

        Self { type_id, process }
    }

    pub fn try_process(&self, item: Option<Box<dyn Any>>) -> Option<Box<dyn Any>> {
        (self.process)(item)
    }
}

struct PrioritizedPipeline {
    pub priority: usize,
    pub pipeline: Pipeline,
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

    pub fn add_pipeline<T: 'static + Send + Sync>(&mut self, pipeline: Pipeline, priority: usize) {
        let type_id = TypeId::of::<T>();
        let entry = self.pipelines.entry(type_id).or_default();
        entry.push(PrioritizedPipeline { priority, pipeline });
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
