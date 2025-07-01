use std::sync::Arc;

use reqwest::{StatusCode, Url, header::HeaderMap};

use crate::request::IronRequest;

#[derive(Debug, Clone)]
pub struct Response {
    /// The final URL of the response
    pub url: Url,

    /// The HTTP status code
    pub status: StatusCode,

    /// Response headers
    pub headers: HeaderMap,

    /// Response body in raw bytes
    pub body: Vec<u8>,

    /// The request that generated this response
    pub request: Arc<IronRequest>,

    /// Protocol used (e.g. "HTTP/1.1", "h2")
    pub protocol: Option<String>,
}

impl Response {
    /// Attempt to decode the body as UTF-8 text
    pub fn text(&self) -> Option<String> {
        String::from_utf8(self.body.clone()).ok()
    }

    /// Get a header by name, if available
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(key).and_then(|val| val.to_str().ok())
    }

    /// Check if the response is a redirect
    pub fn is_redirect(&self) -> bool {
        self.status.is_redirection()
    }

    /// Get content length
    pub fn content_length(&self) -> Option<usize> {
        self.body.len().into()
    }
}
