use crate::request::Request;
use crossbeam::channel::{Receiver, Sender, TryRecvError, unbounded};

// Remove async_trait since these are synchronous operations
pub trait Scheduler: Send + Sync {
    fn dequeue(&mut self) -> Option<Request>;
    fn is_empty(&self) -> bool;
    fn enqueue(&mut self, request: Request) -> Result<(), SchedulerError>;

    // Optional: non-blocking dequeue for better performance
    fn try_dequeue(&mut self) -> Result<Request, TryRecvError> {
        match self.dequeue() {
            Some(req) => Ok(req),
            None => Err(TryRecvError::Empty),
        }
    }
}

#[derive(Debug)]
pub enum SchedulerError {
    ChannelClosed,
    QueueFull,
}

// Simple Scheduler - acts as a FIFO queue
pub struct SimpleScheduler {
    sender: Sender<Request>,
    receiver: Receiver<Request>,
}

impl SimpleScheduler {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self { sender, receiver }
    }

    // Helper method to get a sender handle for external use
    pub fn get_sender(&self) -> Sender<Request> {
        self.sender.clone()
    }
}

impl Default for SimpleScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler for SimpleScheduler {
    fn dequeue(&mut self) -> Option<Request> {
        match self.receiver.try_recv() {
            Ok(request) => Some(request),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }

    fn enqueue(&mut self, request: Request) -> Result<(), SchedulerError> {
        self.sender
            .send(request)
            .map_err(|_| SchedulerError::ChannelClosed)
    }
}
