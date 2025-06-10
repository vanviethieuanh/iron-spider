use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::request::Request;

#[async_trait::async_trait]
pub trait Scheduler: Send + Sync {
    async fn dequeue(&mut self) -> Option<Request>;
    fn is_empty(&self) -> bool;
    fn sender(&self) -> UnboundedSender<Request>;
    fn close_init_sender(&mut self);
}

// NOTE: Simple Scheduler
pub struct SimpleScheduler {
    sender: Option<UnboundedSender<Request>>,
    receiver: UnboundedReceiver<Request>,
}

impl SimpleScheduler {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded_channel();
        Self {
            sender: Some(sender),
            receiver,
        }
    }
}

#[async_trait::async_trait]
impl Scheduler for SimpleScheduler {
    async fn dequeue(&mut self) -> Option<Request> {
        self.receiver.recv().await
    }
    fn sender(&self) -> UnboundedSender<Request> {
        self.sender
            .as_ref()
            .expect("Sender has already been taken/dropped")
            .clone()
    }
    fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }
    fn close_init_sender(&mut self) {
        self.sender.take();
    }
}
