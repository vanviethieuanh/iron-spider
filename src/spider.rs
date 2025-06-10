use std::any::Any;

use crate::{request::Request, response::Response};

pub enum SpiderResult {
    Requests(Vec<Request>),
    Items(Vec<Box<dyn Any>>),
    Both {
        requests: Vec<Request>,
        items: Vec<Box<dyn Any>>,
    },
    None,
}

pub trait Spider: Send + Sync {
    fn start_urls(&self) -> Vec<Request>;
    fn name(&self) -> &str;
    fn parse(response: Response) -> SpiderResult
    where
        Self: Sized;
}
