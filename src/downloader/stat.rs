use dashmap::DashMap;
use reqwest::StatusCode;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct DownloaderStats {
    // Current state
    pub active_requests: usize,
    pub waiting_requests: usize,
    pub peak_concurrent_requests: usize,

    // Totals
    pub total_requests: u64,
    pub total_responses: u64,
    pub total_exceptions: u64,

    // By status code
    pub status_counts: HashMap<u16, u64>,

    // By exception type
    pub exception_counts: HashMap<String, u64>,

    // Data transfer
    pub total_request_bytes: u64,
    pub total_response_bytes: u64,

    // Timing
    pub avg_response_time_ms: f64,
    pub min_response_time_ms: f64,
    pub max_response_time_ms: f64,

    // Rate limiting
    pub rate_limited_count: u64,

    // Rates
    pub request_rate_s: f64,
}

impl Default for DownloaderStats {
    fn default() -> Self {
        Self {
            active_requests: 0,
            waiting_requests: 0,
            peak_concurrent_requests: 0,
            total_requests: 0,
            total_responses: 0,
            total_exceptions: 0,
            status_counts: HashMap::new(),
            exception_counts: HashMap::new(),
            total_request_bytes: 0,
            total_response_bytes: 0,
            avg_response_time_ms: 0.0,
            min_response_time_ms: f64::MAX,
            max_response_time_ms: 0.0,
            rate_limited_count: 0,
            request_rate_s: 0.0,
        }
    }
}

impl fmt::Display for DownloaderStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Downloader Statistics ===")?;
        writeln!(
            f,
            "Active: {}, Waiting: {}, Peak: {}",
            self.active_requests, self.waiting_requests, self.peak_concurrent_requests
        )?;
        writeln!(
            f,
            "Total Requests: {}, Responses: {}, Exceptions: {}",
            self.total_requests, self.total_responses, self.total_exceptions
        )?;
        writeln!(
            f,
            "Data: {} MB sent, {} MB received",
            self.total_request_bytes / 1_000_000,
            self.total_response_bytes / 1_000_000
        )?;
        writeln!(
            f,
            "Response Time: avg={:.1}ms, min={:.1}ms, max={:.1}ms",
            self.avg_response_time_ms, self.min_response_time_ms, self.max_response_time_ms
        )?;

        if !self.status_counts.is_empty() {
            writeln!(f, "Status Codes:")?;
            for (code, count) in &self.status_counts {
                writeln!(f, "  {}: {}", code, count)?;
            }
        }

        if !self.exception_counts.is_empty() {
            writeln!(f, "Exceptions:")?;
            for (exc_type, count) in &self.exception_counts {
                writeln!(f, "  {}: {}", exc_type, count)?;
            }
        }

        Ok(())
    }
}

pub struct StatsTracker {
    // Current state
    active_requests: Arc<AtomicUsize>,
    waiting_requests: Arc<AtomicUsize>,
    peak_queued_requests: Arc<AtomicUsize>,

    // Totals
    total_requests: Arc<AtomicU64>,
    total_responses: Arc<AtomicU64>,
    total_exceptions: Arc<AtomicU64>,

    // Status codes - individual tracking
    status_counts: Arc<DashMap<u16, AtomicU64>>,

    // Exception types - also use DashMap for flexibility
    exception_counts: Arc<DashMap<String, AtomicU64>>,

    // Data transfer
    total_request_bytes: Arc<AtomicU64>,
    total_response_bytes: Arc<AtomicU64>,

    // Timing (we'll track total time for averaging)
    total_response_time_ms: Arc<AtomicU64>,
    min_response_time_ms: Arc<AtomicU64>, // stored as integer milliseconds
    max_response_time_ms: Arc<AtomicU64>,

    // Rate limiting
    rate_limited_count: Arc<AtomicU64>,

    // Start time
    start_time: Arc<Instant>,
}

impl StatsTracker {
    pub fn new() -> Self {
        Self {
            active_requests: Arc::new(AtomicUsize::new(0)),
            waiting_requests: Arc::new(AtomicUsize::new(0)),
            peak_queued_requests: Arc::new(AtomicUsize::new(0)),
            total_requests: Arc::new(AtomicU64::new(0)),
            total_responses: Arc::new(AtomicU64::new(0)),
            total_exceptions: Arc::new(AtomicU64::new(0)),
            status_counts: Arc::new(DashMap::new()),
            exception_counts: Arc::new(DashMap::new()),
            total_request_bytes: Arc::new(AtomicU64::new(0)),
            total_response_bytes: Arc::new(AtomicU64::new(0)),
            total_response_time_ms: Arc::new(AtomicU64::new(0)),
            min_response_time_ms: Arc::new(AtomicU64::new(u64::MAX)),
            max_response_time_ms: Arc::new(AtomicU64::new(0)),
            rate_limited_count: Arc::new(AtomicU64::new(0)),
            start_time: Arc::new(Instant::now()),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.active_requests.load(Ordering::SeqCst) == 0
    }

    pub fn inc_requests(&self, size: u64) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_request_bytes.fetch_add(size, Ordering::Relaxed);
    }

    pub fn inc_waiting(&self) {
        let current = self.waiting_requests.fetch_add(1, Ordering::Relaxed) + 1;
        self.update_peak_queued(current + self.active_requests.load(Ordering::Relaxed));
    }

    pub fn dec_waiting_inc_active(&self) {
        self.waiting_requests.fetch_sub(1, Ordering::Relaxed);
        let current = self.active_requests.fetch_add(1, Ordering::Relaxed) + 1;
        self.update_peak_queued(current + self.waiting_requests.load(Ordering::Relaxed));
    }

    pub fn dec_active(&self) {
        self.active_requests.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn inc_response(&self, status: StatusCode, response_bytes: u64, response_time: Duration) {
        self.total_responses.fetch_add(1, Ordering::Relaxed);
        self.total_response_bytes
            .fetch_add(response_bytes, Ordering::Relaxed);

        // Track individual status code
        let status_code = status.as_u16();
        self.status_counts
            .entry(status_code)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);

        // Track response time
        let response_time_ms = response_time.as_millis() as u64;
        self.total_response_time_ms
            .fetch_add(response_time_ms, Ordering::Relaxed);

        // Update min/max response times
        self.update_min_max_response_time(response_time_ms);
    }

    pub fn inc_exception(&self, error_type: &str) {
        self.total_exceptions.fetch_add(1, Ordering::Relaxed);

        // Track individual exception types
        self.exception_counts
            .entry(error_type.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_rate_limited(&self) {
        self.rate_limited_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_request_bytes(&self, bytes: u64) {
        self.total_request_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    // Helper method to get status count for a specific code
    pub fn get_status_count(&self, status_code: u16) -> u64 {
        self.status_counts
            .get(&status_code)
            .map(|counter| counter.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    // Helper method to get exception count for a specific type
    pub fn get_exception_count(&self, error_type: &str) -> u64 {
        self.exception_counts
            .get(error_type)
            .map(|counter| counter.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    // Get all status codes that have been seen
    pub fn get_all_status_codes(&self) -> Vec<u16> {
        self.status_counts
            .iter()
            .map(|entry| *entry.key())
            .collect()
    }

    // Get all exception types that have been seen
    pub fn get_all_exception_types(&self) -> Vec<String> {
        self.exception_counts
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    fn update_peak_queued(&self, current_total: usize) {
        let mut peak = self.peak_queued_requests.load(Ordering::Relaxed);
        while current_total > peak {
            match self.peak_queued_requests.compare_exchange_weak(
                peak,
                current_total,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => peak = x,
            }
        }
    }

    fn update_min_max_response_time(&self, response_time_ms: u64) {
        // Update minimum
        let mut min = self.min_response_time_ms.load(Ordering::Relaxed);
        while response_time_ms < min {
            match self.min_response_time_ms.compare_exchange_weak(
                min,
                response_time_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => min = x,
            }
        }

        // Update maximum
        let mut max = self.max_response_time_ms.load(Ordering::Relaxed);
        while response_time_ms > max {
            match self.max_response_time_ms.compare_exchange_weak(
                max,
                response_time_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => max = x,
            }
        }
    }

    pub fn get_stats(&self) -> DownloaderStats {
        let total_responses = self.total_responses.load(Ordering::Relaxed);
        let total_response_time = self.total_response_time_ms.load(Ordering::Relaxed);
        let min_time = self.min_response_time_ms.load(Ordering::Relaxed);
        let max_time = self.max_response_time_ms.load(Ordering::Relaxed);

        // Collect all status counts
        let mut status_counts = HashMap::new();
        for entry in self.status_counts.iter() {
            let status_code = *entry.key();
            let count = entry.value().load(Ordering::Relaxed);
            if count > 0 {
                status_counts.insert(status_code, count);
            }
        }

        // Collect all exception counts
        let mut exception_counts = HashMap::new();
        for entry in self.exception_counts.iter() {
            let error_type = entry.key().clone();
            let count = entry.value().load(Ordering::Relaxed);
            if count > 0 {
                exception_counts.insert(error_type, count);
            }
        }

        let total_requests = self.total_requests.load(Ordering::Relaxed);

        DownloaderStats {
            active_requests: self.active_requests.load(Ordering::Relaxed),
            waiting_requests: self.waiting_requests.load(Ordering::Relaxed),
            peak_concurrent_requests: self.peak_queued_requests.load(Ordering::Relaxed),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_responses,
            total_exceptions: self.total_exceptions.load(Ordering::Relaxed),
            status_counts,
            exception_counts,
            total_request_bytes: self.total_request_bytes.load(Ordering::Relaxed),
            total_response_bytes: self.total_response_bytes.load(Ordering::Relaxed),
            avg_response_time_ms: if total_responses > 0 {
                total_response_time as f64 / total_responses as f64
            } else {
                0.0
            },
            min_response_time_ms: if min_time == u64::MAX {
                0.0
            } else {
                min_time as f64
            },
            max_response_time_ms: max_time as f64,
            rate_limited_count: self.rate_limited_count.load(Ordering::Relaxed),
            request_rate_s: (total_requests as f64) / self.start_time.elapsed().as_secs_f64(),
        }
    }
}
