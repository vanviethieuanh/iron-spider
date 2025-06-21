use std::{collections::HashMap, sync::Arc};

use crate::spider::Spider;

#[derive(Clone)]
pub struct Request {
    pub spider: Arc<dyn Spider>,

    pub url: String,
    pub method: reqwest::Method,
    pub headers: Option<reqwest::header::HeaderMap>,
    pub body: Option<String>,
    pub meta: Option<HashMap<String, String>>,
}
