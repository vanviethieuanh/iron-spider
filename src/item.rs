use std::any::Any;

pub type ResultItem = Box<dyn Any + Send + Sync>;
