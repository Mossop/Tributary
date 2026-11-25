//! Tracks whether the dashboard is actively being used.

use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;

pub struct Activity {
    timeout: Duration,
    last_ping: Mutex<Option<Instant>>,
}

impl Activity {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            last_ping: Mutex::new(None),
        }
    }

    pub fn is_active(&self) -> bool {
        is_recent(*self.last_ping.lock().unwrap(), self.timeout)
    }

    /// Records client activity. Returns true if the dashboard was previously
    /// idle.
    pub fn ping(&self) -> bool {
        let mut last = self.last_ping.lock().unwrap();
        let was_active = is_recent(*last, self.timeout);
        *last = Some(Instant::now());
        !was_active
    }
}

fn is_recent(last: Option<Instant>, timeout: Duration) -> bool {
    last.is_some_and(|last| last.elapsed() < timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn becomes_idle_after_timeout() {
        let activity = Activity::new(Duration::from_secs(60));
        assert!(!activity.is_active());

        assert!(activity.ping());
        assert!(activity.is_active());

        tokio::time::advance(Duration::from_secs(30)).await;
        assert!(!activity.ping());
        assert!(activity.is_active());

        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(!activity.is_active());
        assert!(activity.ping());
    }
}
