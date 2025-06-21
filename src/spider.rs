use std::{any::Any, collections::HashMap, sync::Arc};

use reqwest::{Method, header::HeaderMap};
use tracing::debug;

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
    fn parse(&self, response: Response) -> SpiderResult;

    fn close(&self) {
        debug!("Closing spider: {}", self.name());
    }

    fn request(
        &self,
        url: String,
        method: Method,
        headers: Option<HeaderMap>,
        body: Option<String>,
        meta: Option<HashMap<String, String>>,
    ) -> Request
    where
        Self: Sized + Clone + 'static,
    {
        Request {
            spider: Arc::new(self.clone()) as Arc<dyn Spider>,
            url: url,
            method,
            headers,
            body,
            meta,
        }
    }
}
