# Iron Spider
Rust based scraping framework

```rust
fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false) // hides the module path
        .with_thread_names(true)
        .with_writer(std::io::stdout)
        .init();

    let mut http_error_allow_codes = HashSet::new();
    http_error_allow_codes.insert(reqwest::StatusCode::NOT_FOUND);

    let config = EngineConfig {
        downloader_request_timeout: Duration::from_secs(10),
        http_error_allow_codes,
        concurrent_limit: 32,
        tui_stats_interval: Duration::from_millis(500),
        show_tui: false,
        ..Default::default()
    };

    let scheduler = Arc::new(SimpleScheduler::new());
    let example_spider = Arc::new(ExampleSpider::new());
    let spiders: Vec<Arc<dyn Spider>> = (0..10)
        .map(|_| Arc::new((*example_spider).clone()) as Arc<dyn Spider>)
        .collect();

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

    drop(engine);
}
```

Scrapy like spider implementation
```rust
impl Spider for ExampleSpider {
    fn start_requests(&self) -> Vec<Request> {
        (0..30)
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
```
