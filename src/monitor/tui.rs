use crate::{
    config::EngineConfig,
    downloader::{downloader::Downloader, tui::DownloaderWidget},
    pipeline::{manager::PipelineManager, tui::PipelineManagerWidget},
    scheduler::{scheduler::Scheduler, tui::SchedulerWidget},
    spider::{manager::SpiderManager, tui::SpiderManagerWidget},
};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use std::{
    io::stdout,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tui_logger::TuiLoggerWidget;

pub struct TuiMonitor {
    downloader: Arc<Downloader>,
    scheduler: Arc<dyn Scheduler>,
    spider_manager: Arc<SpiderManager>,
    pipeline_manager: Arc<PipelineManager>,

    shutdown_signal: Arc<AtomicBool>,
    config: EngineConfig,
}

impl TuiMonitor {
    pub fn new(
        downloader: Arc<Downloader>,
        scheduler: Arc<dyn Scheduler>,
        spider_manager: Arc<SpiderManager>,
        pipeline_manager: Arc<PipelineManager>,

        shutdown_signal: Arc<AtomicBool>,
        config: EngineConfig,
    ) -> Self {
        Self {
            downloader,
            scheduler,
            pipeline_manager,
            shutdown_signal,
            config,
            spider_manager,
        }
    }

    pub fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        tui_logger::init_logger(self.config.tui_logger_level)
            .expect("failed to initialize tui_logger");
        tracing_subscriber::registry()
            .with(tui_logger::TuiTracingSubscriberLayer)
            .init();

        loop {
            // Get current stats
            let downloader_stats = self.downloader.get_stats();
            let spider_manager_stats = self.spider_manager.get_stats();
            let pipline_manager_stats = self.pipeline_manager.get_stats();
            let scheduler_stats = self.scheduler.get_stats();

            let shutdown_active = self.shutdown_signal.load(Ordering::Relaxed);
            let shutdown_signal = self.shutdown_signal.clone();

            terminal.draw(|f| {
                let vertical_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),
                        Constraint::Percentage(30),
                        Constraint::Percentage(70),
                    ])
                    .split(f.area());

                let top_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                    ])
                    .split(vertical_chunks[1]);

                let title = Paragraph::new(format!(
                    "Spider Dashboard - {} ms",
                    self.config.tui_stats_interval.as_millis()
                ))
                .alignment(Alignment::Center);
                f.render_widget(title, vertical_chunks[0]);

                f.render_widget(
                    SpiderManagerWidget::new(&spider_manager_stats),
                    top_chunks[0],
                );
                f.render_widget(SchedulerWidget::new(&scheduler_stats), top_chunks[1]);
                f.render_widget(DownloaderWidget::new(&downloader_stats), top_chunks[2]);
                f.render_widget(
                    PipelineManagerWidget::new(&pipline_manager_stats),
                    top_chunks[3],
                );

                f.render_widget(
                    TuiLoggerWidget::default()
                        .block(Block::default().title("Logs").borders(Borders::ALL))
                        .style(Style::default().fg(Color::White)),
                    vertical_chunks[2],
                );
            })?;

            // Handle input
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                        shutdown_signal.swap(true, Ordering::Relaxed);
                        break;
                    }
                }
            }

            // Check shutdown
            if shutdown_active {
                break;
            }

            std::thread::sleep(self.config.tui_stats_interval);
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        Ok(())
    }
}
