use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

pub struct Pipeline {
    pub type_id: TypeId,
    pub process: Box<dyn Fn(&dyn Any) + Send + Sync>,
}

impl Pipeline {
    pub fn new<T: 'static + Send + Sync>(handler: impl Fn(&T) + Send + Sync + 'static) -> Self {
        let type_id = TypeId::of::<T>();
        let process = Box::new(move |any: &dyn Any| {
            if let Some(typed) = any.downcast_ref::<T>() {
                handler(typed);
            }
        });

        Self { type_id, process }
    }

    pub fn try_process(&self, item: &dyn Any) {
        (self.process)(item);
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
        if let Some(pipes) = self.pipelines.get(&type_id) {
            for pp in pipes {
                pp.pipeline.try_process(&*item); // Still pass as reference
            }
        } else {
            eprintln!("No pipeline for type {:?}", type_id);
        }
    }
}
