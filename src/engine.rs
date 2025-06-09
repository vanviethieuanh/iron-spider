use std::sync::Arc;

use tokio::task::JoinSet;

use crate::{
    config::Configuration,
    downloader::Downloader,
    pipeline::PipelineManager,
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
        downloader: Downloader,
    ) -> Self {
        let config = config.unwrap_or(Configuration::default());

        Self {
            scheduler,
            spiders,
            pipelines: Arc::new(pipelines),
            config,
            downloader: Arc::new(downloader),
        }
    }

    pub async fn start(&mut self) {
        println!("Starting engine with {} spiders", self.spiders.len(),);

        let scheduler_sender = self.scheduler.sender();
        for spider in &self.spiders {
            for req in spider.start_urls() {
                scheduler_sender
                    .send(req)
                    .expect("Failed to queue request to scheduler.");
            }
        }
        self.scheduler.close();

        let mut join_set = JoinSet::new();
        loop {
            tokio::select! {
                maybe_request = self.scheduler.dequeue() => {
                    match maybe_request {
                        Some(request) => {
                            println!("Dequeued: {}", request.url);

                            let pipelines = Arc::clone(&self.pipelines);
                            let downloader = Arc::clone(&self.downloader);
                            let sender = scheduler_sender.clone();

                            join_set.spawn(async move {
                                let response = downloader.fetch(&request).await;
                                let result = (request.callback)(response);

                                match result {
                                    SpiderResult::Requests(requests) => {
                                        for req in requests {
                                            if sender.send(req).is_err() {
                                                eprintln!("Failed to enqueue request");
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
                                                eprintln!("Failed to enqueue request");
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
                        None => {
                            println!("Scheduler closed and queue is empty");
                        }
                    }
                }

                Some(result) = join_set.join_next() => {
                    match result{
                        Ok(_) => { println!("task finised");
                             if join_set.is_empty() && self.downloader.is_idle() && self.scheduler.is_empty() {
                                println!("empty");
                                break;
                            }

                        },
                        Err(_) => todo!(),
                    }
                }

                else => {
                    break;
                }
            }
        }
        println!("Engine finished crawling.");
    }
}
