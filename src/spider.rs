use std::{any::Any, collections::HashSet};

use reqwest::StatusCode;

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
    fn name(&self) -> &str;
    fn start_urls(&self) -> Vec<Request>;
    fn parse(response: Response) -> SpiderResult
    where
        Self: Sized;
}
