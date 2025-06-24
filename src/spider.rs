use std::{any::Any, collections::HashMap, sync::Arc};

use async_trait::async_trait;
use reqwest::{Method, Url, header::HeaderMap};
use tracing::debug;

use crate::{request::Request, response::Response};

pub type ResultItem = Vec<Box<dyn Any>>;

pub enum SpiderResult {
    Requests(Vec<Request>),
    Items(Vec<ResultItem>),
    Both {
        requests: Vec<Request>,
        items: Vec<ResultItem>,
    },
    None,
}

#[async_trait]
pub trait Spider: Send + Sync {
    fn name(&self) -> &str;
    fn start_urls(&self) -> Vec<Request>;

    async fn parse(&self, response: Response) -> SpiderResult;

    async fn close(&self) {
        debug!("Closing spider: {}", self.name());
    }

    fn request(
        &self,
        url: Url,
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
