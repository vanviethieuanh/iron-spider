use std::sync::Arc;

use reqwest::Client;
use tokio::{sync::mpsc::UnboundedSender, task::JoinSet};
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
    scheduler: Box<dyn Scheduler>,
    spiders: Vec<Box<dyn Spider>>,
    pipelines: Arc<PipelineManager>,
    config: Configuration,
    downloader: Arc<Downloader>,
}

impl Engine {
    pub fn new(
        scheduler: Box<dyn Scheduler>,
        spiders: Vec<Box<dyn Spider>>,
        pipelines: PipelineManager,
        config: Option<Configuration>,
    ) -> Self {
        let config = config.unwrap_or_default();

        let downloader_request_quota = config.downloader_request_quota;

        Self {
            scheduler,
            spiders,
            pipelines: Arc::new(pipelines),
            config,
            downloader: Arc::new(Downloader::new(Client::new(), downloader_request_quota)),
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

    async fn handle_request(
        request: Request,
        downloader: Arc<Downloader>,
        pipelines: Arc<PipelineManager>,
        sender: UnboundedSender<Request>,
    ) {
        let response = downloader.fetch(&request).await;
        let result = (request.callback)(response);

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
    }

    pub async fn start(&mut self) {
        info!("Starting engine with {} spiders", self.spiders.len());

        let scheduler_sender = self.scheduler.sender();
        self.enqueue_start_urls();
        self.scheduler.close_init_sender();

        let mut task_set = JoinSet::new();
        loop {
            tokio::select! {
                maybe_request = self.scheduler.dequeue() => {
                    match maybe_request {
                        Some(request) => {
                            debug!("Dequeued: {}", request.url);

                            let pipelines = Arc::clone(&self.pipelines);
                            let downloader = Arc::clone(&self.downloader);
                            let sender = scheduler_sender.clone();

                            task_set.spawn(Self::handle_request(request, downloader, pipelines, sender));
                        }
                        None => {
                            info!("Scheduler closed and queue is empty");
                        }
                    }
                }

                Some(result) = task_set.join_next() => {
                    match result {
                        Ok(_) => {
                            debug!("Task finished");
                            if task_set.is_empty() && self.downloader.is_idle() && self.scheduler.is_empty() {
                                info!("No more tasks, downloader idle, and scheduler empty — exiting loop");
                                break;
                            }
                        },
                        Err(e) => {
                            error!("Task failed: {:?}", e);
                        },
                    }
                }

                else => {
                    break;
                }
            }
        }
        info!("Engine finished crawling.");
    }
}
