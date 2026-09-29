use std::time::Duration;

pub(super) const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const STALL_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const HEALTHY_RESET_INTERVAL: Duration = Duration::from_secs(20);
const RETRY_DELAYS: [Duration; 6] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ReconnectBackoff {
    retry_index: usize,
}

impl ReconnectBackoff {
    pub(super) fn next_delay(&mut self) -> Duration {
        let delay = RETRY_DELAYS[self.retry_index.min(RETRY_DELAYS.len() - 1)];
        self.retry_index = (self.retry_index + 1).min(RETRY_DELAYS.len() - 1);
        delay
    }

    pub(super) fn reset(&mut self) {
        self.retry_index = 0;
    }

    pub(super) fn has_retried(&self) -> bool {
        self.retry_index > 0
    }
}

pub(super) fn stream_is_stalled(
    frame_count: u64,
    elapsed_since_start: Duration,
    elapsed_since_progress: Duration,
) -> bool {
    (frame_count == 0 && elapsed_since_start >= STARTUP_TIMEOUT)
        || (frame_count > 0 && elapsed_since_progress >= STALL_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_advances_caps_and_resets() {
        let mut backoff = ReconnectBackoff::default();
        let seconds = (0..8)
            .map(|_| backoff.next_delay().as_secs())
            .collect::<Vec<_>>();
        assert_eq!(seconds, [1, 2, 5, 10, 15, 30, 30, 30]);
        assert!(backoff.has_retried());
        backoff.reset();
        assert!(!backoff.has_retried());
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn watchdog_boundaries_distinguish_startup_and_active_streams() {
        assert!(!stream_is_stalled(
            0,
            STARTUP_TIMEOUT - Duration::from_nanos(1),
            Duration::ZERO
        ));
        assert!(stream_is_stalled(0, STARTUP_TIMEOUT, Duration::ZERO));
        assert!(!stream_is_stalled(
            1,
            Duration::from_secs(60),
            STALL_TIMEOUT - Duration::from_nanos(1)
        ));
        assert!(stream_is_stalled(1, Duration::from_secs(60), STALL_TIMEOUT));
    }
}
