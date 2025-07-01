use crate::{
    config::EngineConfig,
    downloader::{downloader::Downloader, tui::DownloaderWidget},
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
    layout::{Constraint, Direction, Layout},
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
            let downloader_stats = self.downloader.get_stats();
            let spider_manager_stats = self.spider_manager.get_stats();
            let scheduler_stats = self.scheduler.lock().unwrap().get_stats();

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

                let spider_widget = SpiderManagerWidget::new(&spider_manager_stats);
                f.render_widget(spider_widget, chunks[0]);

                let scheduler_widget = SchedulerWidget::new(&scheduler_stats);
                f.render_widget(scheduler_widget, chunks[1]);

                let spider_manager_widget = DownloaderWidget::new(&downloader_stats);
                f.render_widget(spider_manager_widget, chunks[2]);
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
