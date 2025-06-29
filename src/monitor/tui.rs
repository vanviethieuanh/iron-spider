// tui_monitor.rs
use crate::{config::EngineConfig, downloader::downloader::Downloader, scheduler::Scheduler};
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
    config: EngineConfig,
}

impl TuiMonitor {
    pub fn new(
        downloader: Arc<Downloader>,
        scheduler: Arc<Mutex<Box<dyn Scheduler>>>,
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
            let idle_time = self.last_activity.lock().unwrap().elapsed();
            let shutdown_active = self.shutdown_signal.load(Ordering::Relaxed);
            let shutdown_signal = self.shutdown_signal.clone();

            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(f.area());

                // Downloader info
                let downloader_text = format!(
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
                    Request Rate: {:.2}(r/s)",
                    stats.active_requests,
                    stats.waiting_requests,
                    stats.peak_concurrent_requests,
                    stats.total_requests,
                    stats.total_responses,
                    stats.total_exceptions,
                    stats.avg_response_time_ms,
                    stats.min_response_time_ms,
                    stats.max_response_time_ms,
                    stats.rate_limited_count,
                    stats.total_request_bytes,
                    stats.total_response_bytes,
                    stats.request_rate_s
                );

                let downloader_paragraph = Paragraph::new(downloader_text)
                    .block(
                        Block::default()
                            .title("Downloader Stats")
                            .borders(Borders::ALL),
                    )
                    .style(Style::default().fg(Color::White));
                f.render_widget(downloader_paragraph, chunks[0]);

                // Scheduler info
                let scheduler_text = format!(
                    "Queue Empty: {}\n\
                    \n\
                    Idle Time: {:.1}s\n\
                    Idle Timeout: {:.1}s\n\
                    \n\
                    Shutdown Signal: {}\n\
                    \n\
                    Status Codes:\n{}",
                    scheduler_empty,
                    idle_time.as_secs_f64(),
                    self.config.idle_timeout.as_secs_f64(),
                    shutdown_active,
                    format_status_codes(&stats.status_counts)
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

fn format_status_codes(status_counts: &std::collections::HashMap<u16, u64>) -> String {
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
