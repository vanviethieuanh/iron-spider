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
        let total = self.stats.total_spiders.max(1) as f64;

        let text = format!(
            "Total Spiders       : {:>5}\n\
             Pending             : {:>5} ({:>5.2}%)\n\
             Active              : {:>5} ({:>5.2}%)\n\
             Closed              : {:>5} ({:>5.2}%)\n\
             Dropped Responses   : {:>5}",
            self.stats.total_spiders,
            self.stats.pending_spiders,
            self.stats.pending_spiders as f64 / total * 100.0,
            self.stats.active_spiders,
            self.stats.active_spiders as f64 / total * 100.0,
            self.stats.closed_spiders,
            self.stats.closed_spiders as f64 / total * 100.0,
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
