use std::fmt;

pub struct SchedulerStats {
    pub items_count: u64,
}

impl fmt::Display for SchedulerStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Queue Items: {}", self.items_count)
    }
}
