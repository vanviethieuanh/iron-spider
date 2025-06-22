use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use iron_spider::{
    config::Configuration,
    engine::Engine,
    pipeline::{FnPipeline, Pipeline, PipelineManager},
    request::Request,
    response::Response,
    scheduler::SimpleScheduler,
    spider::{Spider, SpiderResult},
};
use regex::Regex;
use reqwest::Url;
use scraper::{Html, Selector};
use tracing::{Level, info};

#[derive(Debug)]
pub struct ArticleItem {
    pub title: String,
    pub author: String,
}

#[derive(Clone)]
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

#[async_trait]
impl Spider for ExampleSpider {
    fn start_urls(&self) -> Vec<Request> {
        (1..=1000)
            .map(|i| {
                let url = format!("http://localhost:5000/article/{}", 3)
                    .parse::<Url>()
                    .expect("Invalid URL");
                self.request(url, reqwest::Method::GET, None, None, None)
            })
            .collect()
    }

    fn name(&self) -> &str {
        "example_spider"
    }

    async fn parse(&self, response: Response) -> SpiderResult {
        if let Some(item) = ExampleSpider::parse_article_html(&response.body) {
            match extract_number(item.title.as_str()) {
                Some(i) => {
                    if i != 1 {
                        self.mark_discovered(response.request.url.to_string());

                        let next_url_str = format!("./article/{}", i - 1);
                        let next_url = response
                            .request
                            .url
                            .join(&next_url_str)
                            .expect("Invalid next URL");

                        SpiderResult::Both {
                            requests: vec![self.request(
                                next_url,
                                reqwest::Method::GET,
                                None,
                                None,
                                None,
                            )],
                            items: vec![Box::new(item)],
                        }
                    } else {
                        self.mark_discovered(response.request.url.to_string());
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
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO) // Or "debug" for more verbose logs
        .init();

    let scheduler = Box::new(SimpleScheduler::new());
    let example_spider = Arc::new(ExampleSpider::new());
    let spiders: Vec<Box<dyn Spider>> = vec![Box::new((*example_spider).clone())];

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

    let mut pipeline_manager = PipelineManager::new();
    pipeline_manager.add_pipeline::<ArticleItem>(print_article_pipe, 30);
    pipeline_manager.add_pipeline::<ArticleItem>(transform_article_pipe, 10);

    let mut http_error_allow_codes = HashSet::new();
    http_error_allow_codes.insert(reqwest::StatusCode::NOT_FOUND);

    let config = Some(Configuration {
        downloader_request_timeout: Duration::from_secs(10),
        http_error_allow_codes,
        ..Default::default()
    });

    let mut engine = Engine::new(scheduler, spiders, pipeline_manager, config);
    engine.start().await;

    info!(
        "Discovered: {} url(s)",
        example_spider.discovered_count().to_string().as_str()
    );
}
