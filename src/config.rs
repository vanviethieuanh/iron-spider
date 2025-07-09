use std::{collections::HashSet, time::Duration};

use governor::Quota;
use reqwest::StatusCode;
use tui_logger::LevelFilter;

#[derive(Clone)]
pub struct EngineConfig {
    pub downloader_request_timeout: Duration,
    pub downloader_connection_timeout: Duration,
    pub downloader_delay: Duration,
    pub downloader_request_quota: Option<Quota>,
    pub store_cookies: bool,
    // TODO: implement download data quota
    pub user_agent: Option<String>,

    pub http_error_allow_codes: HashSet<StatusCode>,
    pub concurrent_limit: usize,

    pub pipeline_worker_threads: usize,
    pub spider_manager_worker_threads: usize,
    pub tui_stats_interval: Duration,

    // Shutdown when: no active requests AND scheduler is empty AND idle timeout
    pub idle_timeout: Duration,
    pub tui_logger_level: LevelFilter,

    pub show_tui: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        let allowed = HashSet::new();

        Self {
            downloader_request_timeout: Duration::from_secs(3),
            downloader_connection_timeout: Duration::from_secs(3),
            downloader_delay: Duration::ZERO,
            downloader_request_quota: None,
            user_agent: Some("IronSpider/0.0.1".to_string()),
            http_error_allow_codes: allowed,
            concurrent_limit: 32,

            pipeline_worker_threads: 4,
            tui_stats_interval: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(1),

            tui_logger_level: LevelFilter::Info,

            show_tui: false,
            spider_manager_worker_threads: 4,
            store_cookies: true,
        }
    }
}
