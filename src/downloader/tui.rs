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
}

impl<'a> Widget for DownloaderWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = self.stats.to_string();

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
