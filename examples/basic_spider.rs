use iron_spider::{
    config::Configuration,
    engine::Engine,
    pipeline::{Pipeline, PipelineManager},
    request::Request,
    response::Response,
    scheduler::SimpleScheduler,
    spider::{Spider, SpiderResult},
};
use regex::Regex;
use scraper::{Html, Selector};
use tracing::{Level, info};

#[derive(Debug)]
pub struct ArticleItem {
    pub title: String,
    pub author: String,
}

pub struct ExampleSpider;

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
}

impl Spider for ExampleSpider {
    fn start_urls(&self) -> Vec<Request> {
        vec![
            Request {
                url: "http://localhost:5000/article/4".to_string(),
                method: reqwest::Method::GET,
                headers: None,
                body: None,
                meta: None,
                callback: ExampleSpider::parse,
            },
            Request {
                url: "http://localhost:5000/article/5".to_string(),
                method: reqwest::Method::GET,
                headers: None,
                body: None,
                meta: None,
                callback: ExampleSpider::parse,
            },
            Request {
                url: "http://localhost:5000/article/3".to_string(),
                method: reqwest::Method::GET,
                headers: None,
                body: None,
                meta: None,
                callback: ExampleSpider::parse,
            },
        ]
    }

    fn name(&self) -> &str {
        "example_spider"
    }

    fn parse(response: Response) -> SpiderResult {
        if let Some(item) = Self::parse_article_html(&response.body) {
            match extract_number(item.title.as_str()) {
                Some(i) => {
                    if i != 1 {
                        SpiderResult::Both {
                            requests: vec![Request {
                                url: format!("http://localhost:5000/article/{}", i - 1),
                                method: reqwest::Method::GET,
                                headers: None,
                                body: None,
                                meta: None,
                                callback: ExampleSpider::parse,
                            }],
                            items: vec![Box::new(item)],
                        }
                    } else {
                        SpiderResult::Items(vec![Box::new(item)])
                    }
                }
                None => SpiderResult::None,
            }
        } else {
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
    let spiders: Vec<Box<dyn Spider>> = vec![Box::new(ExampleSpider)];

    let print_article_pipe = Pipeline::new(|item: Option<ArticleItem>| {
        info!("Article item pipeline: {:?}", item);
        item
    });
    let transform_article_pipe = Pipeline::new(|item: Option<ArticleItem>| {
        info!("Transforming item: {:?}", item);
        let transformed = item.map(|mut i| {
            i.author = "Transformed author".to_string();
            i
        });

        transformed
    });

    let mut pipeline_manager = PipelineManager::new();
    pipeline_manager.add_pipeline::<ArticleItem>(print_article_pipe, 30);
    pipeline_manager.add_pipeline::<ArticleItem>(transform_article_pipe, 10);

    let mut engine = Engine::new(
        scheduler,
        spiders,
        pipeline_manager,
        Some(Configuration {
            download_delay_ms: 1000,
            user_agent: Some("IronSpider/0.1".into()),
            downloader_request_quota: None,
        }),
    );
    engine.start().await;
}
