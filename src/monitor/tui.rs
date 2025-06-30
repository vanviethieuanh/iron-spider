use crate::{
    config::EngineConfig,
    downloader::{downloader::Downloader, tui::DownloaderWidget},
    scheduler::scheduler::Scheduler,
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
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use std::{
    io::stdout,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub struct TuiMonitor {
    downloader: Arc<Downloader>,
    scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
    shutdown_signal: Arc<AtomicBool>,
    last_activity: Arc<Mutex<Instant>>,
    spider_manager: Arc<SpiderManager>,
    config: EngineConfig,
}

impl TuiMonitor {
    pub fn new(
        downloader: Arc<Downloader>,
        scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
        spider_manager: Arc<SpiderManager>,
        shutdown_signal: Arc<AtomicBool>,
        last_activity: Arc<Mutex<Instant>>,
        config: EngineConfig,
    ) -> Self {
        Self {
            downloader,
            scheduler,
            shutdown_signal,
            last_activity,
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

        loop {
            // Get current stats
            let stats = self.downloader.get_stats();
            let scheduler_empty = self.scheduler.lock().unwrap().is_empty();
            let scheduler_counter = self.scheduler.lock().unwrap().count();
            let idle_time = self.last_activity.lock().unwrap().elapsed();
            let shutdown_active = self.shutdown_signal.load(Ordering::Relaxed);
            let shutdown_signal = self.shutdown_signal.clone();

            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(33),
                        Constraint::Percentage(33),
                        Constraint::Percentage(34),
                    ])
                    .split(f.area());

                let widget = DownloaderWidget::new(&stats);
                f.render_widget(widget, chunks[2]);

                let spider_stats = self.spider_manager.get_stats(); // assuming Arc<SpiderManagerStatsTracker>
                let spider_widget = SpiderManagerWidget::new(&spider_stats);
                f.render_widget(spider_widget, chunks[0]); // choose the appropriate `chunks[n]`

                let scheduler_text = format!(
                    "Queue Empty: {}\n\
                    Queue Items: {}\n\
                    \n\
                    Idle Time: {:.1}s\n\
                    Idle Timeout: {:.1}s\n\
                    \n\
                    Shutdown Signal: {}",
                    scheduler_empty,
                    scheduler_counter,
                    idle_time.as_secs_f64(),
                    self.config.idle_timeout.as_secs_f64(),
                    shutdown_active,
                );

                let scheduler_color = if scheduler_empty {
                    Color::Green
                } else {
                    Color::Yellow
                };
                let scheduler_paragraph = Paragraph::new(scheduler_text)
                    .block(
                        Block::default()
                            .title(format!(
                                "Scheduler & System - Interval {} second(s)",
                                self.config.stats_interval.as_secs_f64()
                            ))
                            .borders(Borders::ALL),
                    )
                    .style(Style::default().fg(scheduler_color));
                f.render_widget(scheduler_paragraph, chunks[1]);
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

            std::thread::sleep(Duration::from_millis(500));
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        Ok(())
    }
}
