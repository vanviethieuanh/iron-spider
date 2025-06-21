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
        }
    }
}
