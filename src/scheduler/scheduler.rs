use crate::request::IronRequest;
use crossbeam::channel::{Receiver, Sender, TryRecvError, unbounded};

// Remove async_trait since these are synchronous operations
pub trait Scheduler: Send + Sync {
    fn dequeue(&mut self) -> Option<IronRequest>;
    fn is_empty(&self) -> bool;
    fn enqueue(&mut self, request: IronRequest) -> Result<(), SchedulerError>;
    fn count(&self) -> u64;

    // Optional: non-blocking dequeue for better performance
    fn try_dequeue(&mut self) -> Result<IronRequest, TryRecvError> {
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
    sender: Sender<IronRequest>,
    receiver: Receiver<IronRequest>,
}

impl SimpleScheduler {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self { sender, receiver }
    }
}

impl Default for SimpleScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler for SimpleScheduler {
    fn dequeue(&mut self) -> Option<IronRequest> {
        match self.receiver.try_recv() {
            Ok(request) => Some(request),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }

    fn enqueue(&mut self, request: IronRequest) -> Result<(), SchedulerError> {
        self.sender
            .send(request)
            .map_err(|_| SchedulerError::ChannelClosed)
    }

    /// Returns the approximate number of waiting requests in the scheduler.
    /// This may be slightly off due to concurrent operations.
    fn count(&self) -> u64 {
        self.receiver.len() as u64
    }
}
