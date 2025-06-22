use std::{collections::HashMap, sync::Arc};

use reqwest::Url;

use crate::spider::Spider;

#[derive(Clone)]
pub struct Request {
    pub spider: Arc<dyn Spider>,

    pub url: Url,
    pub method: reqwest::Method,
    pub headers: Option<reqwest::header::HeaderMap>,
    pub body: Option<String>,
    pub meta: Option<HashMap<String, String>>,
}
