use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
    time::Duration,
};

use iron_spider::{
    config::EngineConfig,
    engine::Engine,
    pipeline::{fn_pipeline::FnPipeline, manager::PipelineManager},
    request::{Request, RequestBuilder},
    response::Response,
    scheduler::SimpleScheduler,
    spider::spider::{Spider, SpiderResult},
};
use regex::Regex;
use reqwest::Url;
use scraper::{Html, Selector};
use tracing::info;

#[derive(Debug)]
pub struct ArticleItem {
    pub title: String,
    pub author: String,
}

#[derive(Clone, Debug)]
pub struct ExampleSpider {
    pub discovered: Arc<RwLock<HashSet<String>>>,
}

fn extract_number(s: &str) -> Option<u32> {
    let re = Regex::new(r"\d+").ok()?;
    let caps = re.find(s)?;
    caps.as_str().parse::<u32>().ok()
}

impl ExampleSpider {
    fn parse_article_html(html: &str) -> Option<ArticleItem> {
        let document = Html::parse_document(html);
        let article_selector = Selector::parse("article").ok()?;
        let author_selector = Selector::parse("article > author").ok()?;

        let article_element = document.select(&article_selector).next()?;
        let author_element = document.select(&author_selector).next()?;

        let article_text = article_element
            .text()
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string();
        let author_text = author_element
            .text()
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string();

        let title = article_text.replace(&author_text, "").trim().to_string();

        Some(ArticleItem {
            title: title,
            author: author_text,
        })
    }

    pub fn new() -> Self {
        Self {
            discovered: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Optional: add helper method to insert safely
    pub fn mark_discovered(&self, url: String) {
        let mut guard = self.discovered.write().unwrap();
        guard.insert(url);
    }

    /// Optional: check existence
    pub fn is_discovered(&self, url: &str) -> bool {
        let guard = self.discovered.read().unwrap();
        guard.contains(url)
    }

    pub fn discovered_count(&self) -> usize {
        let guard = self.discovered.read().unwrap();
        guard.len()
    }
}

impl Spider for ExampleSpider {
    fn start_requests(&self) -> Vec<Request> {
        (1..10000)
            .map(|_| {
                let url = format!("http://127.0.0.1:5000/article/{}", 3)
                    .parse::<Url>()
                    .expect("Invalid URL");

                RequestBuilder::new()
                    .url(url)
                    .method(reqwest::Method::GET)
                    .build()
                    .expect("Failed to build request")
            })
            .collect()
    }

    fn name(&self) -> &str {
        "example_spider"
    }

    fn parse(&self, response: Response) -> SpiderResult {
        if let Some(item) = response
            .text()
            .as_deref()
            .and_then(ExampleSpider::parse_article_html)
        {
            match extract_number(item.title.as_str()) {
                Some(i) => {
                    self.mark_discovered(response.url.to_string());

                    if i != 1 {
                        let next_url_str = format!("./article/{}", i - 1);
                        let next_url = response.url.join(&next_url_str).expect("Invalid next URL");

                        let next_request = RequestBuilder::new()
                            .url(next_url)
                            .method(reqwest::Method::GET)
                            .build()
                            .expect("Failed to build next request");

                        SpiderResult::Both {
                            requests: vec![next_request],
                            items: vec![Box::new(item)],
                        }
                    } else {
                        SpiderResult::Items(vec![Box::new(item)])
                    }
                }
                None => SpiderResult::None,
            }
        } else {
            info!("Empty response");
            SpiderResult::None
        }
    }

    fn close(&self) {
        info!("Heyyyyyy, I'm leaving!!!");
    }
}

fn main() {
    // tracing_subscriber::fmt()
    //     .with_max_level(Level::INFO) // Or "debug" for more verbose logs
    //     .init();

    let mut http_error_allow_codes = HashSet::new();
    http_error_allow_codes.insert(reqwest::StatusCode::NOT_FOUND);

    let config = EngineConfig {
        downloader_request_timeout: Duration::from_secs(10),
        http_error_allow_codes,
        concurrent_limit: 32,
        ..Default::default()
    };

    let scheduler = Box::new(SimpleScheduler::new());
    let example_spider = Arc::new(ExampleSpider::new());
    let spiders: Vec<Arc<dyn Spider>> = vec![Arc::new((*example_spider).clone())];

    let print_article_pipe = FnPipeline::new(|item: Option<ArticleItem>| {
        info!("Article item pipeline: {:?}", item);
        item
    });

    let transform_article_pipe = FnPipeline::new(|item: Option<ArticleItem>| {
        info!("Transforming item: {:?}", item);
        item.map(|mut i| {
            i.author = "Transformed author".to_string();
            i
        })
    });

    let mut pipeline_manager = PipelineManager::new(&config);
    pipeline_manager.add_pipeline::<ArticleItem>(print_article_pipe, 30);
    pipeline_manager.add_pipeline::<ArticleItem>(transform_article_pipe, 10);

    let mut engine = Engine::new(scheduler, spiders, pipeline_manager, Some(config));
    let _ = engine.start();

    info!(
        "Discovered: {} url(s)",
        example_spider.discovered_count().to_string().as_str()
    );
}
