use std::collections::HashMap;

#[derive(Clone)]
pub struct Request {
    pub url: String,
    pub method: reqwest::Method,
    pub headers: Option<reqwest::header::HeaderMap>,
    pub body: Option<String>,
    pub meta: Option<HashMap<String, String>>,
    pub callback: fn(super::response::Response) -> super::spider::SpiderResult,
}
