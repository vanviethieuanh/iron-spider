use std::{collections::HashMap, sync::Arc};

use reqwest::Url;

use crate::spider::manager::RegisteredSpider;

#[derive(Clone, Debug)]
pub struct Request {
    pub url: Url,
    pub method: reqwest::Method,
    pub headers: Option<reqwest::header::HeaderMap>,
    pub body: Option<String>,

    pub meta: Option<HashMap<String, String>>,
}

impl Request {
    pub fn size(&self) -> u64 {
        let mut total = 0u64;

        // === Request line: "METHOD /path HTTP/1.1\r\n" ===
        let method = self.method.as_str();
        let path = self.url.path(); // does not include domain
        total += method.len() as u64 + 1; // + space
        total += path.len() as u64 + 1; // + space
        total += "HTTP/1.1\r\n".len() as u64;

        // === Headers ===
        if let Some(headers) = &self.headers {
            for (key, value) in headers.iter() {
                total += key.as_str().len() as u64;
                total += 2; // ": "
                total += value.as_bytes().len() as u64;
                total += 2; // "\r\n"
            }
        }

        // === End of headers ===
        total += 2; // "\r\n"

        // === Body ===
        if let Some(body) = &self.body {
            total += body.as_bytes().len() as u64;
        }

        total
    }
}

#[derive(Clone, Debug)]
pub struct IronRequest {
    pub registered_spider: Arc<RegisteredSpider>,
    pub request: Request,
}

#[derive(Debug, Default)]
pub struct RequestBuilder {
    url: Option<Url>,
    method: reqwest::Method,
    headers: Option<reqwest::header::HeaderMap>,
    body: Option<String>,
    meta: Option<HashMap<String, String>>,
}

impl RequestBuilder {
    pub fn new() -> Self {
        Self {
            method: reqwest::Method::GET,
            ..Default::default()
        }
    }

    pub fn url(mut self, url: Url) -> Self {
        self.url = Some(url);
        self
    }

    pub fn method(mut self, method: reqwest::Method) -> Self {
        self.method = method;
        self
    }

    pub fn headers(mut self, headers: reqwest::header::HeaderMap) -> Self {
        self.headers = Some(headers);
        self
    }

    pub fn body<T: Into<String>>(mut self, body: T) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn meta(mut self, meta: HashMap<String, String>) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Finalize into a plain `Request`
    pub fn build(self) -> Result<Request, &'static str> {
        Ok(Request {
            url: self.url.ok_or("Missing URL")?,
            method: self.method,
            headers: self.headers,
            body: self.body,
            meta: self.meta,
        })
    }

    /// Finalize into an `IronRequest` with a known spider
    pub fn build_with_spider(
        self,
        spider: Arc<RegisteredSpider>,
    ) -> Result<IronRequest, &'static str> {
        Ok(IronRequest {
            registered_spider: spider,
            request: self.build()?,
        })
    }
}
