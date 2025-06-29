use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::spider::stat::SpiderManagerStats;

pub struct SpiderManagerWidget<'a> {
    stats: &'a SpiderManagerStats,
}

impl<'a> SpiderManagerWidget<'a> {
    pub fn new(stats: &'a SpiderManagerStats) -> Self {
        Self { stats }
    }
}

impl<'a> Widget for SpiderManagerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = format!(
            "Total Spiders: {}\n\
             Active Spiders: {}\n\
             Sleeping Spiders: {}\n\
             Dropped Responses: {}",
            self.stats.total_spiders,
            self.stats.active_spiders,
            self.stats.sleeping_spiders,
            self.stats.dropped_responses,
        );

        Paragraph::new(text)
            .block(
                Block::default()
                    .title("Spider Manager Stats")
                    .borders(Borders::ALL),
            )
            .style(Style::default().fg(Color::Cyan))
            .render(area, buf);
    }
}
