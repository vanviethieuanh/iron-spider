use governor::Quota;

#[derive(Clone)]
pub struct Configuration {
    pub download_delay_ms: u64,
    pub downloader_request_quota: Option<Quota>,
    // TODO: implement download data quota
    pub user_agent: Option<String>,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            download_delay_ms: 0,
            downloader_request_quota: None,
            user_agent: Some(
                "IronSpider/0.1 (+https://github.com/vanviethieuanh/iron-spider)".to_string(),
            ),
        }
    }
}
