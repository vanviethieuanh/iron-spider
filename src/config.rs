#[derive(Clone)]
pub struct Configuration {
    pub download_delay_ms: u64,
    pub user_agent: Option<String>,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            download_delay_ms: 0,
            user_agent: Some(
                "IronSpider/0.1 (+https://github.com/vanviethieuanh/iron-spider)".to_string(),
            ),
        }
    }
}
