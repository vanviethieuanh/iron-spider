use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::downloader::stat::DownloaderStats;

pub struct DownloaderWidget<'a> {
    stats: &'a DownloaderStats,
}

impl<'a> DownloaderWidget<'a> {
    pub fn new(stats: &'a DownloaderStats) -> Self {
        Self { stats }
    }

    fn format_status_codes(&self) -> String {
        let status_counts = self.stats.status_counts.clone();

        if status_counts.is_empty() {
            return "None".to_string();
        }

        let total: u64 = status_counts.values().sum();
        let mut sorted_codes: Vec<_> = status_counts.iter().collect();
        sorted_codes.sort_by_key(|(code, _)| *code);

        sorted_codes
            .iter()
            .map(|(code, count)| {
                let percentage = if total > 0 {
                    (**count as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                format!("  {}: {} ({:.1}%)", code, count, percentage)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl<'a> Widget for DownloaderWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = format!(
            "Active Requests: {}\n\
             Waiting Requests: {}\n\
             Peak Concurrent: {}\n\
             \n\
             Total Requests: {}\n\
             Total Responses: {}\n\
             Total Exceptions: {}\n\
             \n\
             Avg Response Time: {:.2}ms\n\
             Min Response Time: {:.2}ms\n\
             Max Response Time: {:.2}ms\n\
             \n\
             Rate Limited: {}\n\
             Request Bytes: {}\n\
             Response Bytes: {}\n\
             \n\
             Request Rate: {:.2}(r/s)
             \n\
             Status Codes:\n{}",
            self.stats.active_requests,
            self.stats.waiting_requests,
            self.stats.peak_concurrent_requests,
            self.stats.total_requests,
            self.stats.total_responses,
            self.stats.total_exceptions,
            self.stats.avg_response_time_ms,
            self.stats.min_response_time_ms,
            self.stats.max_response_time_ms,
            self.stats.rate_limited_count,
            self.stats.total_request_bytes,
            self.stats.total_response_bytes,
            self.stats.request_rate_s,
            self.format_status_codes()
        );

        Paragraph::new(text)
            .block(
                Block::default()
                    .title("Downloader Stats")
                    .borders(Borders::ALL),
            )
            .style(Style::default().fg(Color::White))
            .render(area, buf);
    }
}
