use std::any::TypeId;

use crate::item::ResultItem;

pub trait Pipeline: Send + Sync {
    fn type_id(&self) -> TypeId;
    fn try_process(&self, item: Option<ResultItem>) -> Option<ResultItem>;
    fn close(&self);
}
