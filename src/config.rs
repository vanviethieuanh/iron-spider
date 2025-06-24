use std::{collections::HashSet, time::Duration};

use governor::Quota;
use reqwest::StatusCode;

#[derive(Clone)]
pub struct Configuration {
    pub downloader_request_timeout: Duration,
    pub downloader_delay: Duration,
    pub downloader_request_quota: Option<Quota>,
    // TODO: implement download data quota
    pub user_agent: Option<String>,

    pub http_error_allow_codes: HashSet<StatusCode>,
    pub concurrent_limit: usize,

    pub(crate) pipeline_worker_threads: i32,
    pub(crate) downloader_threads: i32,
    pub(crate) shutdown_timeout: Duration,
    pub(crate) stats_interval: Duration,

    // Shutdown when: no active requests AND scheduler is empty AND idle timeout
    pub(crate) idle_timeout: Duration,
}

impl Default for Configuration {
    fn default() -> Self {
        let allowed = HashSet::new();

        Self {
            downloader_request_timeout: Duration::from_secs(3),
            downloader_delay: Duration::ZERO,
            downloader_request_quota: None,
            user_agent: Some("IronSpider/0.0.1".to_string()),
            http_error_allow_codes: allowed,
            concurrent_limit: 32,

            pipeline_worker_threads: 4,
            downloader_threads: 1,
            shutdown_timeout: Duration::from_secs(1),
            stats_interval: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(1),
        }
    }
}
