use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use reqwest::StatusCode;
use tracing::error;

use crate::request::Request;
use crate::response::Response;

#[derive(Clone)]
pub struct Downloader {
    client: reqwest::Client,
    active_requests: Arc<AtomicUsize>,
    limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
    http_error_allow_codes: HashSet<StatusCode>,
}

impl Downloader {
    pub fn new(
        client: reqwest::Client,
        quota: Option<Quota>,
        http_error_allow_codes: HashSet<StatusCode>,
    ) -> Self {
        Self {
            client,
            active_requests: Arc::new(AtomicUsize::new(0)),
            limiter: match quota {
                Some(q) => Some(Arc::new(RateLimiter::direct(q))),
                None => None,
            },
            http_error_allow_codes,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.active_requests.load(Ordering::SeqCst) == 0
    }

    pub async fn fetch(&self, request: &Request) -> Option<Response> {
        if let Some(ref limiter) = self.limiter {
            let _permit = limiter.until_ready().await;
        }
        self.active_requests.fetch_add(1, Ordering::SeqCst);

        let resp = self
            .client
            .request(request.method.clone(), request.url.clone())
            .headers(request.headers.clone().unwrap_or_default())
            .body(request.body.clone().unwrap_or_default())
            .send()
            .await;

        self.active_requests.fetch_sub(1, Ordering::SeqCst);

        match resp {
            Ok(r) => {
                let status = r.status();
                let url = r.url().clone();

                if r.error_for_status_ref().is_ok() || self.http_error_allow_codes.contains(&status)
                {
                    let body = r.text().await.unwrap_or_default();
                    Some(Response {
                        status,
                        body,
                        request: request.clone(),
                    })
                } else {
                    error!("Status error [{}]: {}", status, url);
                    None
                }
            }
            Err(e) => {
                if e.is_timeout() {
                    error!("Timeout: {} -> {}", request.url, e);
                } else if e.is_connect() {
                    error!(
                        "Connection error (maybe server not started): {} -> {}",
                        request.url, e
                    );
                } else if e.is_request() {
                    error!("Bad request formation: {} -> {}", request.url, e);
                } else if e.is_body() {
                    error!("Body error: {} -> {}", request.url, e);
                } else {
                    error!("Request failed (other): {} -> {:?}", request.url, e);
                }

                None
            }
        }
    }
}
