use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crossbeam::channel::Sender;
use governor::RateLimiter;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use reqwest::{Client, StatusCode};
use tokio::sync::Semaphore;
use tracing::error;

use crate::config::EngineConfig;
use crate::errors::EngineError;
use crate::response::Response;
use crate::scheduler::Scheduler;

#[derive(Clone)]
pub struct Downloader {
    client: reqwest::Client,
    limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
    http_error_allow_codes: HashSet<StatusCode>,
    download_sem: Arc<Semaphore>,

    active_requests: Arc<AtomicUsize>,
    waiting_requests: Arc<AtomicUsize>,
}

impl Downloader {
    pub fn new(engine_config: &EngineConfig) -> Result<Self, EngineError> {
        let http_error_allow_codes = engine_config.http_error_allow_codes.clone();
        let limiter = match engine_config.downloader_request_quota {
            Some(q) => Some(Arc::new(RateLimiter::direct(q))),
            None => None,
        };

        let mut client_builder = Client::builder()
            .timeout(engine_config.downloader_request_timeout)
            .connect_timeout(Duration::from_millis(200));
        if let Some(ref user_agent) = engine_config.user_agent {
            client_builder = client_builder.user_agent(user_agent);
        }
        let client = client_builder.build().map_err(|e| {
            EngineError::InitializationError(format!(
                "Error while building downloader client: {}",
                e
            ))
        })?;

        Ok(Self {
            client,
            limiter,
            http_error_allow_codes,
            download_sem: Arc::new(Semaphore::new(engine_config.concurrent_limit)),

            active_requests: Arc::new(AtomicUsize::new(0)),
            waiting_requests: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub fn is_idle(&self) -> bool {
        self.active_requests.load(Ordering::SeqCst) == 0
    }

    // Start a async runtime for downlaoder
    pub fn start(
        &self,
        scheduler: Arc<std::sync::Mutex<Box<dyn Scheduler>>>,
        resp_sender: Sender<Response>,
        active_requests: Arc<AtomicUsize>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<std::sync::Mutex<Instant>>,
        config: &EngineConfig,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

        rt.block_on(async {
            let mut join_set = tokio::task::JoinSet::new();

            while !shutdown_signal.load(Ordering::Relaxed) {
                // Pull request from scheduler
                let request = {
                    let mut sched = scheduler.lock().unwrap();
                    sched.dequeue()
                };

                match request {
                    Some(req) => {
                        self.waiting_requests.fetch_add(1, Ordering::Relaxed);
                        *last_activity.lock().unwrap() = Instant::now();

                        let engine_active_requests = Arc::clone(&active_requests);

                        let client = self.client.clone();
                        let resp_sender = resp_sender.clone();
                        let config = config.clone();
                        let download_sem = self.download_sem.clone();
                        let http_error_allow_codes = self.http_error_allow_codes.clone();
                        let waiting_requests = self.waiting_requests.clone();
                        let active_requests = self.active_requests.clone();
                        let limiter = self.limiter.clone();

                        // Perform HTTP request
                        join_set.spawn(async move {
                            if let Some(limiter) = limiter.as_ref() {
                                limiter.until_ready().await;
                            }
                            let _permit = download_sem.acquire().await.unwrap();

                            engine_active_requests.fetch_add(1, Ordering::Relaxed);
                            waiting_requests.fetch_sub(1, Ordering::Relaxed);
                            active_requests.fetch_add(1, Ordering::Relaxed);

                            let req_clone = req.clone();
                            let resp = client
                                .request(req.method, req.url)
                                .headers(req.headers.unwrap_or_default())
                                .body(req.body.unwrap_or_default())
                                .send()
                                .await;

                            active_requests.fetch_sub(1, Ordering::Relaxed);
                            engine_active_requests.fetch_sub(1, Ordering::Relaxed);
                            drop(_permit);

                            match resp {
                                Ok(r) => {
                                    let status = r.status();
                                    let url = r.url().clone();

                                    if r.error_for_status_ref().is_ok()
                                        || http_error_allow_codes.contains(&status)
                                    {
                                        let body = r.text().await.unwrap_or_default();
                                        let _ = resp_sender.send(Response {
                                            status,
                                            body,
                                            request: req_clone,
                                        });
                                    } else {
                                        error!("Status error [{}]: {}", status, url);
                                    }
                                }
                                Err(e) => {
                                    if e.is_timeout() {
                                        error!("Timeout: {} -> {}", req_clone.url, e);
                                    } else if e.is_connect() {
                                        error!(
                                            "Connection error (maybe server not started): {} -> {}",
                                            req_clone.url, e
                                        );
                                    } else if e.is_request() {
                                        error!("Bad request formation: {} -> {}", req_clone.url, e);
                                    } else if e.is_body() {
                                        error!("Body error: {} -> {}", req_clone.url, e);
                                    } else {
                                        error!(
                                            "Request failed (other): {} -> {:?}",
                                            req_clone.url, e
                                        );
                                    }
                                }
                            }
                        });
                    }
                    None => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }
        });

        println!("⬇️  Downloader thread stopped");
    }
}
