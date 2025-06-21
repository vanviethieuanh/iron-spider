use std::sync::Arc;

use reqwest::Client;
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

use crate::{
    config::Configuration,
    downloader::Downloader,
    pipeline::PipelineManager,
    request::Request,
    scheduler::Scheduler,
    spider::{Spider, SpiderResult},
};

pub struct Engine {
    scheduler: Box<dyn Scheduler + Send + Sync>,
    spiders: Vec<Box<dyn Spider>>,
    pipelines: Arc<PipelineManager>,
    config: Configuration,
    downloader: Arc<Downloader>,
    tasks: JoinSet<()>,
}

impl Engine {
    pub fn new(
        scheduler: Box<dyn Scheduler + Send + Sync>,
        spiders: Vec<Box<dyn Spider>>,
        pipelines: PipelineManager,
        config: Option<Configuration>,
    ) -> Self {
        let config = config.unwrap_or_default();

        let downloader_client = Client::builder()
            .timeout(config.downloader_request_timeout)
            .connector_layer(tower::limit::concurrency::ConcurrencyLimitLayer::new(
                config.concurrent_limit,
            ))
            .build()
            .expect("Failed to build downloader's client.");
        let downloader_request_quota = config.downloader_request_quota;
        let http_error_allow_codes = config.http_error_allow_codes.clone();

        Self {
            scheduler,
            spiders,
            pipelines: Arc::new(pipelines),
            config,
            downloader: Arc::new(Downloader::new(
                downloader_client,
                downloader_request_quota,
                http_error_allow_codes,
            )),
            tasks: JoinSet::new(),
        }
    }

    fn enqueue_start_urls(&self) {
        let sender = self.scheduler.sender();
        for spider in &self.spiders {
            for req in spider.start_urls() {
                if let Err(e) = sender.send(req) {
                    error!("Failed to queue request to scheduler: {:?}", e);
                }
            }
        }
    }

    fn spawn_handle_request(&mut self, request: Request) {
        let pipelines = Arc::clone(&self.pipelines);
        let downloader = Arc::clone(&self.downloader);
        let sender = self.scheduler.sender();

        self.tasks.spawn(async move {
            let response = downloader.fetch(&request).await;
            let result = match response {
                Some(resp) => request.spider.parse(resp),
                None => SpiderResult::None,
            };

            match result {
                SpiderResult::Requests(requests) => {
                    for req in requests {
                        if sender.send(req).is_err() {
                            warn!("Failed to enqueue request");
                        }
                    }
                }
                SpiderResult::Items(items) => {
                    for item in items {
                        pipelines.process_item(item);
                    }
                }
                SpiderResult::Both { requests, items } => {
                    for req in requests {
                        if sender.send(req).is_err() {
                            warn!("Failed to enqueue request");
                        }
                    }
                    for item in items {
                        pipelines.process_item(item);
                    }
                }
                SpiderResult::None => {}
            }
        });
    }

    pub fn stop(&self) {
        info!("Stopping engine!");
        for spider in &self.spiders {
            spider.close();
        }
    }

    pub fn completed(&self) -> bool {
        self.tasks.is_empty() && self.downloader.is_idle() && self.scheduler.is_empty()
    }

    pub async fn start(&mut self) {
        info!("Starting engine with {} spiders", self.spiders.len());

        self.enqueue_start_urls();
        loop {
            tokio::select! {
                maybe_request = self.scheduler.dequeue() => {
                    match maybe_request {
                        Some(request) => {
                            debug!("Dequeued: {}", request.url);
                            self.spawn_handle_request(request);
                        }
                        None => {
                            if self.completed() {
                                info!("No more tasks, downloader idle, and scheduler empty — exiting loop");
                                self.stop();
                                break;
                            }
                        }
                    }
                }

                Some(result) = self.tasks.join_next() => {
                    match result {
                        Ok(_) => {
                            debug!("Task finished");
                        },
                        Err(e) => {
                            error!("Task failed: {:?}", e);
                        },
                    }

                    if self.completed() {
                        info!("No more tasks, downloader idle, and scheduler empty — exiting loop");
                        self.stop();
                        break;
                    }
                }
            }
        }

        self.scheduler.close_init_sender();
        info!("Engine finished crawling.");
    }
}
