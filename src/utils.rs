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
