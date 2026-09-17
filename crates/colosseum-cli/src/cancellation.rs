//! One cancellation path for every durable driver.
//!
//! Interrupting a long run is a supported operation, not an accident. The
//! first interrupt stops launching new units and gives the ones in flight a
//! bounded period to finish on their own; the second abandons them at once.
//! Either way the driver returns normally, its checkpoint is written and the
//! run directory resumes, so nothing already paid for is thrown away.
//!
//! There is deliberately one implementation. A driver that grew its own stop
//! handling would drift from the others in exactly the way the durable-run
//! contract exists to prevent.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Seconds an in-flight game is given to finish after a stop is requested.
pub const DEFAULT_STOP_GRACE_SECONDS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CancelStage {
    /// No stop has been requested.
    Running,
    /// Launch nothing new; in-flight units may still finish.
    Stopping,
    /// Abandon in-flight units immediately.
    Abandon,
}

/// A shared, cloneable handle to one run's stop state.
#[derive(Debug, Clone)]
pub struct Cancellation {
    stage: Arc<watch::Sender<CancelStage>>,
    grace: Duration,
    remaining_units: Option<Arc<AtomicU64>>,
}

impl Cancellation {
    #[must_use]
    pub fn new(grace: Duration) -> Self {
        Self {
            stage: Arc::new(watch::channel(CancelStage::Running).0),
            grace,
            remaining_units: None,
        }
    }

    /// Request a clean stop once this many units have been committed.
    ///
    /// This is a second trigger for the same path, not a second path: an
    /// interrupt and an exhausted budget are indistinguishable downstream,
    /// which is what makes the interrupt behaviour testable without one.
    #[must_use]
    pub fn with_unit_budget(mut self, units: u64) -> Self {
        self.remaining_units = Some(Arc::new(AtomicU64::new(units)));
        if units == 0 {
            self.request_stop();
        }
        self
    }

    /// Count one committed unit against any budget.
    pub fn record_committed_unit(&self) {
        let Some(remaining) = &self.remaining_units else {
            return;
        };
        let left = remaining
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                Some(value.saturating_sub(1))
            })
            .unwrap_or(0);
        if left <= 1 {
            self.request_stop();
        }
    }

    /// A handle that is never triggered, for callers with no interrupt source.
    #[must_use]
    pub fn inactive() -> Self {
        Self::new(Duration::from_secs(DEFAULT_STOP_GRACE_SECONDS))
    }

    #[must_use]
    pub fn stage(&self) -> CancelStage {
        *self.stage.borrow()
    }

    /// True once new units must not be launched.
    #[must_use]
    pub fn stopping(&self) -> bool {
        self.stage() >= CancelStage::Stopping
    }

    /// Ask for a clean stop. Repeating it does not escalate.
    pub fn request_stop(&self) {
        self.stage.send_if_modified(|stage| {
            if *stage == CancelStage::Running {
                *stage = CancelStage::Stopping;
                true
            } else {
                false
            }
        });
    }

    /// Escalate to abandoning in-flight units now.
    pub fn request_abandon(&self) {
        self.stage.send_if_modified(|stage| {
            if *stage == CancelStage::Abandon {
                false
            } else {
                *stage = CancelStage::Abandon;
                true
            }
        });
    }

    /// Resolve when in-flight units must be abandoned: the grace period after
    /// a stop expired, or a second interrupt arrived. This never resolves
    /// while the run is healthy, so a driver can select on it every iteration.
    pub async fn abandon(&self) {
        let mut stage = self.stage.subscribe();
        while *stage.borrow_and_update() == CancelStage::Running {
            if stage.changed().await.is_err() {
                // The sender outlives every driver; treat a closed channel as
                // "no stop will ever arrive" rather than as a stop.
                return std::future::pending().await;
            }
        }
        if *stage.borrow() == CancelStage::Abandon {
            return;
        }
        let grace = tokio::time::sleep(self.grace);
        tokio::pin!(grace);
        loop {
            tokio::select! {
                () = &mut grace => return,
                changed = stage.changed() => {
                    if changed.is_err() {
                        return std::future::pending().await;
                    }
                    if *stage.borrow_and_update() == CancelStage::Abandon {
                        return;
                    }
                }
            }
        }
    }

    /// Listen for console interrupts for as long as the returned task lives.
    ///
    /// The first interrupt requests a clean stop, every later one abandons the
    /// work in flight. On Windows this is the console control event; on Unix
    /// it is `SIGINT`.
    #[must_use]
    pub fn listen_for_interrupts(&self) -> InterruptListener {
        let cancellation = self.clone();
        let grace = self.grace;
        InterruptListener(tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    return;
                }
                if cancellation.stage() == CancelStage::Running {
                    cancellation.request_stop();
                    eprintln!(
                        "stopping: no new work will be launched; games in flight have {} seconds to finish (interrupt again to abandon them)",
                        grace.as_secs()
                    );
                } else {
                    cancellation.request_abandon();
                    eprintln!("abandoning games in flight; the run directory can be resumed");
                }
            }
        }))
    }
}

/// Stops listening when dropped.
#[derive(Debug)]
pub struct InterruptListener(JoinHandle<()>);

impl Drop for InterruptListener {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_healthy_run_never_reaches_the_abandon_point() {
        let cancellation = Cancellation::new(Duration::from_millis(10));
        assert!(!cancellation.stopping());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), cancellation.abandon())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_clean_stop_abandons_only_after_the_grace_period() {
        let cancellation = Cancellation::new(Duration::from_millis(200));
        cancellation.request_stop();
        assert!(cancellation.stopping());
        assert_eq!(cancellation.stage(), CancelStage::Stopping);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), cancellation.abandon())
                .await
                .is_err(),
            "in-flight work was abandoned before its grace period"
        );
        tokio::time::timeout(Duration::from_millis(500), cancellation.abandon())
            .await
            .expect("the bounded grace period must expire");
    }

    #[tokio::test]
    async fn a_second_interrupt_abandons_without_waiting() {
        let cancellation = Cancellation::new(Duration::from_secs(3_600));
        cancellation.request_stop();
        let escalate = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            escalate.request_abandon();
        });
        tokio::time::timeout(Duration::from_millis(500), cancellation.abandon())
            .await
            .expect("a second interrupt must not wait for the grace period");
        assert_eq!(cancellation.stage(), CancelStage::Abandon);
    }

    #[tokio::test]
    async fn an_exhausted_unit_budget_requests_the_same_clean_stop() {
        let cancellation = Cancellation::new(Duration::from_millis(50)).with_unit_budget(2);
        cancellation.record_committed_unit();
        assert!(!cancellation.stopping(), "stopped one unit early");
        cancellation.record_committed_unit();
        assert_eq!(cancellation.stage(), CancelStage::Stopping);
        // Extra units after the budget cannot escalate to abandonment.
        cancellation.record_committed_unit();
        assert_eq!(cancellation.stage(), CancelStage::Stopping);
    }

    #[tokio::test]
    async fn a_run_without_a_budget_is_never_stopped_by_committing_units() {
        let cancellation = Cancellation::new(Duration::from_millis(50));
        for _ in 0..100 {
            cancellation.record_committed_unit();
        }
        assert!(!cancellation.stopping());
    }

    #[tokio::test]
    async fn repeating_a_stop_does_not_escalate_it() {
        let cancellation = Cancellation::new(Duration::from_millis(200));
        cancellation.request_stop();
        cancellation.request_stop();
        assert_eq!(cancellation.stage(), CancelStage::Stopping);
    }
}
