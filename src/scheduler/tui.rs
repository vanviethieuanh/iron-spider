use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::scheduler::stat::SchedulerStats;

pub struct SchedulerWidget<'a> {
    pub stats: &'a SchedulerStats,
}

impl<'a> SchedulerWidget<'a> {
    pub fn new(stats: &'a SchedulerStats) -> Self {
        Self { stats }
    }
}

impl<'a> Widget for SchedulerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let color = if self.stats.items_count > 0 {
            Color::Green
        } else {
            Color::Yellow
        };

        let text = format!("Queue Items      : {}", self.stats.items_count);

        Paragraph::new(text)
            .block(Block::default().title("Scheduler").borders(Borders::ALL))
            .style(Style::default().fg(color))
            .render(area, buf);
    }
}
