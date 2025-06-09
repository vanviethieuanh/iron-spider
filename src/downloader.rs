use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::request::Request;
use crate::response::Response;

#[derive(Clone)]
pub struct Downloader {
    client: reqwest::Client,
    active_requests: Arc<AtomicUsize>,
}

impl Downloader {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            active_requests: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.active_requests.load(Ordering::SeqCst) == 0
    }

    pub async fn fetch(&self, request: &Request) -> Response {
        self.active_requests.fetch_add(1, Ordering::SeqCst);

        let resp = self
            .client
            .request(request.method.clone(), &request.url)
            .headers(request.headers.clone().unwrap_or_default())
            .body(request.body.clone().unwrap_or_default())
            .send()
            .await;

        self.active_requests.fetch_sub(1, Ordering::SeqCst);

        match resp {
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                Response {
                    status,
                    body,
                    request: request.clone(),
                }
            }
            Err(e) => {
                // Optionally: log or retry
                Response {
                    status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                    body: format!("Download error: {}", e),
                    request: request.clone(),
                }
            }
        }
    }
}
