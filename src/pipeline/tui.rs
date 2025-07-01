use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::pipeline::stat::PipelineManagerStats;

pub struct PipelineManagerWidget<'a> {
    pub stats: &'a PipelineManagerStats,
}

impl<'a> PipelineManagerWidget<'a> {
    pub fn new(stats: &'a PipelineManagerStats) -> Self {
        Self { stats }
    }
}

impl<'a> Widget for PipelineManagerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = self.stats.to_string();

        Paragraph::new(text)
            .block(
                Block::default()
                    .title("Pipeline Manager")
                    .borders(Borders::ALL),
            )
            .style(Style::default().fg(Color::LightYellow))
            .render(area, buf);
    }
}
