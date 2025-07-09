use std::time::Duration;

pub fn human_duration(d: Duration) -> String {
    let total_secs = d.as_secs();

    let days = total_secs / 86_400;
    let hours = (total_secs % 86_400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    let mut parts = vec![];

    if days > 0 {
        parts.push(format!("{} days", days));
    }
    if hours > 0 {
        parts.push(format!("{} h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{} min", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{} s", seconds));
    }

    parts.join(" ")
}

pub fn human_number(n: u64) -> String {
    const THOUSAND: f64 = 1_000.0;
    const MILLION: f64 = 1_000_000.0;
    const BILLION: f64 = 1_000_000_000.0;
    const TRILLION: f64 = 1_000_000_000_000.0;
    const QUADRILLION: f64 = 1_000_000_000_000_000.0;

    match n {
        n if n >= QUADRILLION as u64 => format!("{:.2}Q", n as f64 / QUADRILLION),
        n if n >= TRILLION as u64 => format!("{:.2}T", n as f64 / TRILLION),
        n if n >= BILLION as u64 => format!("{:.2}B", n as f64 / BILLION),
        n if n >= MILLION as u64 => format!("{:.2}M", n as f64 / MILLION),
        n if n >= THOUSAND as u64 => format!("{:.2}K", n as f64 / THOUSAND),
        _ => n.to_string(),
    }
}
