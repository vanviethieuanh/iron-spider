use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam::channel::Sender;
use governor::RateLimiter;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use reqwest::{Client, StatusCode};
use tokio::sync::Semaphore;
use tracing::{error, info};

use crate::config::EngineConfig;
use crate::downloader::stat::{DownloaderStats, DownloaderStatsTracker};
use crate::errors::EngineError;
use crate::response::Response;
use crate::scheduler::scheduler::Scheduler;

static WAITING_FACTOR: usize = 2;

#[derive(Clone)]
pub struct Downloader {
    client: reqwest::Client,
    limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
    http_error_allow_codes: HashSet<StatusCode>,
    download_sem: Arc<Semaphore>,

    stats_tracker: Arc<DownloaderStatsTracker>,

    // Max waiting request this downloader will hold on tokio runtime.
    max_waiting: usize,
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
            .connect_timeout(engine_config.downloader_connection_timeout);
        if let Some(ref user_agent) = engine_config.user_agent {
            client_builder = client_builder.user_agent(user_agent);
        }
        let client = client_builder.build().map_err(|e| {
            EngineError::InitializationError(format!(
                "Error while building downloader client: {}",
                e
            ))
        })?;
        let max_waiting = engine_config.concurrent_limit * WAITING_FACTOR;

        Ok(Self {
            client,
            limiter,
            http_error_allow_codes,
            download_sem: Arc::new(Semaphore::new(engine_config.concurrent_limit)),

            stats_tracker: Arc::new(DownloaderStatsTracker::new()),
            max_waiting,
        })
    }

    pub fn get_stats(&self) -> DownloaderStats {
        self.stats_tracker.get_stats()
    }

    pub fn is_idle(&self) -> bool {
        self.stats_tracker.is_idle()
    }

    // Start a async runtime for downlaoder
    pub fn start(
        &self,
        scheduler: Arc<dyn Scheduler>,
        resp_sender: Sender<Response>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<std::sync::Mutex<Instant>>,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

        rt.block_on(async {
            let mut join_set = tokio::task::JoinSet::new();

            while !shutdown_signal.load(Ordering::Relaxed) {
                if self.stats_tracker.get_waiting() >= self.max_waiting {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }

                match scheduler.dequeue() {
                    Some(iron_req) => {
                        self.stats_tracker.inc_waiting();
                        *last_activity.lock().unwrap() = Instant::now();

                        let client = self.client.clone();
                        let resp_sender = resp_sender.clone();
                        let download_sem = self.download_sem.clone();
                        let http_error_allow_codes = self.http_error_allow_codes.clone();
                        let limiter = self.limiter.clone();
                        let stats_tracker = self.stats_tracker.clone();

                        // Perform HTTP request
                        join_set.spawn(async move {
                            if let Some(limiter) = limiter.as_ref() {
                                limiter.until_ready().await;
                            }
                            let _permit = download_sem.acquire().await.unwrap();

                            stats_tracker.dec_waiting_inc_active();

                            let actual_request = iron_req.request.clone();
                            let request_url = actual_request.url.clone();

                            stats_tracker.inc_requests(actual_request.size());
                            let start_time = Instant::now();
                            let resp = client
                                .request(actual_request.method, actual_request.url)
                                .headers(actual_request.headers.unwrap_or_default())
                                .body(actual_request.body.unwrap_or_default())
                                .send()
                                .await;
                            let response_time = start_time.elapsed();

                            drop(_permit);
                            stats_tracker.dec_active();

                            match resp {
                                Ok(r) => {
                                    let status = r.status();
                                    let is_status_accepted = r.error_for_status_ref().is_ok()
                                        || http_error_allow_codes.contains(&status);

                                    let url = r.url().clone();
                                    let headers = r.headers().clone();
                                    let version = format!("{:?}", r.version()); // e.g., HTTP/1.1, H2
                                    let body = r.bytes().await.unwrap_or_default().to_vec();
                                    let body_len = body.len();

                                    if is_status_accepted {
                                        let _ = resp_sender.send(Response {
                                            url,
                                            status,
                                            headers,
                                            body,
                                            request: Arc::new(iron_req),
                                            protocol: Some(version),
                                        });
                                    } else {
                                        error!("Status error [{}]: {}", status, url);
                                    }

                                    stats_tracker.inc_response(
                                        status,
                                        body_len as u64,
                                        response_time,
                                    );
                                }
                                Err(e) => {
                                    if e.is_timeout() {
                                        error!("Timeout: {} -> {}", request_url, e);
                                        stats_tracker.inc_exception("timeout");
                                    } else if e.is_connect() {
                                        error!(
                                            "Connection error (maybe server not started): {} -> {}",
                                            request_url, e
                                        );
                                        stats_tracker.inc_exception("connect");
                                    } else if e.is_request() {
                                        error!("Bad request formation: {} -> {}", request_url, e);
                                        stats_tracker.inc_exception("request");
                                    } else if e.is_body() {
                                        error!("Body error: {} -> {}", request_url, e);
                                        stats_tracker.inc_exception("body");
                                    } else {
                                        error!(
                                            "Request failed (other): {} -> {:?}",
                                            request_url, e
                                        );
                                        stats_tracker.inc_exception("unkown");
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

        info!("⬇️  Downloader thread stopped");
    }
}
