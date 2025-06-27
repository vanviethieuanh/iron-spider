use std::any::{Any, TypeId};

use crate::{item::ResultItem, pipeline::pipeline::Pipeline};

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
