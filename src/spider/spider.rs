use crate::{item::ResultItem, request::Request, response::Response};
use std::{
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
};
use tracing::debug;

#[derive(Debug)]
pub struct SpiderState {
    // in-flight request counts
    pub in_flight_requests: AtomicUsize,

    // in-flight parsing
    pub in_flight_parse: AtomicUsize,

    // Count of all created requests
    pub created_requests: AtomicUsize,
}

impl SpiderState {
    pub fn new() -> Self {
        Self {
            in_flight_requests: AtomicUsize::new(0),
            in_flight_parse: AtomicUsize::new(0),
            created_requests: AtomicUsize::new(0),
        }
    }

    pub fn is_activated(&self) -> bool {
        let requests = self.in_flight_requests.load(Ordering::Relaxed);
        let parsing = self.in_flight_parse.load(Ordering::Relaxed);

        requests + parsing > 0
    }
}

pub enum SpiderResult {
    Requests(Vec<Request>),
    Items(Vec<ResultItem>),
    Both {
        requests: Vec<Request>,
        items: Vec<ResultItem>,
    },
    None,
}

pub trait Spider: Send + Sync + Debug {
    fn name(&self) -> &str;
    fn start_requests(&self) -> Vec<Request>;
    fn parse(&self, response: Response) -> SpiderResult;
    fn close(&self) {
        debug!("Closing spider: {}", self.name());
    }
}
