use anyhow::{anyhow, Result};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Shared cancellation and a single deadline for an entire instruction.
#[derive(Clone, Default)]
pub struct ExecutionControl {
    cancelled: Arc<AtomicBool>,
    deadline: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Cancelled,
    TimedOut,
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "execution cancelled",
            Self::TimedOut => "execution timed out",
        })
    }
}

impl std::error::Error for StopReason {}

impl ExecutionControl {
    pub fn with_timeout(timeout: Duration) -> Result<Self> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| anyhow!("execution timeout is too large"))?;
        Ok(Self {
            deadline: Some(deadline),
            ..Self::default()
        })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn stop_reason(&self) -> Option<StopReason> {
        if self.cancelled.load(Ordering::Acquire) {
            Some(StopReason::Cancelled)
        } else if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            Some(StopReason::TimedOut)
        } else {
            None
        }
    }

    /// Remaining deadline budget for APIs that accept their own timeout.
    pub fn remaining_time(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub fn check(&self) -> std::result::Result<(), StopReason> {
        match self.stop_reason() {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }

    /// Wait cooperatively, checking cancellation and the deadline at least every 20ms.
    pub fn wait(&self, duration: Duration) -> std::result::Result<(), StopReason> {
        let started = Instant::now();
        loop {
            self.check()?;
            let remaining = duration.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(());
            }
            let deadline_remaining = self
                .deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(remaining);
            std::thread::sleep(
                remaining
                    .min(deadline_remaining)
                    .min(Duration::from_millis(20)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_cancellation() {
        let control = ExecutionControl::default();
        control.clone().cancel();
        assert_eq!(control.check(), Err(StopReason::Cancelled));
        assert_eq!(
            control.wait(Duration::from_secs(10)),
            Err(StopReason::Cancelled)
        );
    }

    #[test]
    fn wait_observes_cancellation_from_another_thread() {
        let control = ExecutionControl::default();
        let other = control.clone();
        let cancelling = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            other.cancel();
        });
        let started = Instant::now();
        let result = control.wait(Duration::from_secs(10));
        cancelling.join().unwrap();
        assert_eq!(result, Err(StopReason::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn clones_keep_the_original_deadline() {
        let control = ExecutionControl::with_timeout(Duration::ZERO).unwrap();
        assert_eq!(control.clone().check(), Err(StopReason::TimedOut));
        assert_eq!(
            control.wait(Duration::from_secs(10)),
            Err(StopReason::TimedOut)
        );
    }
}
