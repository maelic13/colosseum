//! Where the time of one search went.
//!
//! A search is a round trip: the harness writes `go`, the engine searches and
//! reports, and its `bestmove` travels back through the pipe to the game task.
//! The clock charges one interval for all of it. When that interval exceeds
//! what the engine itself says it spent, the difference was lost somewhere in
//! the round trip, and only stamps taken at each step can say where.
//!
//! Every stamp is taken by the thread that performs its step: the game task
//! stamps the `go` before writing it, the return of that write, and the moment
//! it takes a line off the channel; the pipe reader thread stamps each line as
//! it arrives. Recording them costs a copied `Instant` per step and nothing on
//! the reader thread beyond the stamp it already took.

use std::time::{Duration, Instant};

/// The monotonic stamps of one search, from `go` to the game task holding its
/// `bestmove`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchTiming {
    /// Stamped by the game task just before the `go` (or `stop`, or
    /// `ponderhit`) is written. The charged interval starts here.
    pub go_stamped: Instant,
    /// Stamped by the game task when the write and its flush returned.
    pub write_returned: Instant,
    /// When the search must have answered: the charged interval may not end
    /// after this.
    pub deadline: Instant,
    /// The first `info` line of this search, as the reader thread stamped it.
    pub first_info: Option<Instant>,
    /// The last `info` line of this search, as the reader thread stamped it.
    pub last_info: Option<Instant>,
    /// The `time` field of that last `info` line, when it carried one.
    pub last_info_time_ms: Option<u64>,
    /// The last `time` the engine reported during the search: its own account
    /// of how long it searched.
    pub engine_time_ms: Option<u64>,
    /// The `bestmove` line, as the reader thread stamped it. The charged
    /// interval ends here.
    pub bestmove_arrived: Option<Instant>,
    /// When the game task took the `bestmove` line off the channel.
    pub consumed: Option<Instant>,
    /// The `bestmove` arrived after the deadline, and was read only to learn
    /// when it came.
    pub late: bool,
    /// The engine's clock started at an earlier command than the charge did:
    /// `go ponder` before `ponderhit`, or `go` before `stop`. Its reported
    /// time then shares no origin with the charged interval, and neither the
    /// overhead nor the last `info`'s lag can be computed from the two.
    pub earlier_origin: bool,
}

/// One phase of a search's round trip, named for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundTripPhase {
    /// `go` stamped → its write returned: the harness writing to the pipe.
    GoWrite,
    /// Write returned → first `info` arrived: the engine starting to search,
    /// and its first report crossing the pipe.
    ToFirstInfo,
    /// First `info` → last `info`: the engine searching.
    BetweenInfo,
    /// Last `info` → `bestmove` arrived: the engine finishing, and its answer
    /// crossing the pipe.
    LastInfoToBestmove,
    /// `bestmove` arrived → consumed: the game task getting round to it. Not
    /// charged; recorded because a slow task is a harness fact worth seeing.
    BestmoveToConsumed,
}

impl SearchTiming {
    /// A search whose `go` was stamped at `go_stamped` and written by
    /// `write_returned`, due by `deadline`.
    #[must_use]
    pub fn new(go_stamped: Instant, write_returned: Instant, deadline: Instant) -> Self {
        Self {
            go_stamped,
            write_returned,
            deadline,
            first_info: None,
            last_info: None,
            last_info_time_ms: None,
            engine_time_ms: None,
            bestmove_arrived: None,
            consumed: None,
            late: false,
            earlier_origin: false,
        }
    }

    /// Record an `info` line that arrived at `arrived`, carrying `time_ms`.
    pub fn info(&mut self, arrived: Instant, time_ms: Option<u64>) {
        self.first_info.get_or_insert(arrived);
        self.last_info = Some(arrived);
        self.last_info_time_ms = time_ms;
        if time_ms.is_some() {
            self.engine_time_ms = time_ms;
        }
    }

    /// How long a phase took, when both of its ends were observed.
    #[must_use]
    pub fn phase(&self, phase: RoundTripPhase) -> Option<Duration> {
        let (from, to) = match phase {
            RoundTripPhase::GoWrite => (Some(self.go_stamped), Some(self.write_returned)),
            RoundTripPhase::ToFirstInfo => (Some(self.write_returned), self.first_info),
            RoundTripPhase::BetweenInfo => (self.first_info, self.last_info),
            RoundTripPhase::LastInfoToBestmove => (self.last_info, self.bestmove_arrived),
            RoundTripPhase::BestmoveToConsumed => (self.bestmove_arrived, self.consumed),
        };
        Some(to?.saturating_duration_since(from?))
    }

    /// An instant as an offset from the `go` stamp.
    #[must_use]
    pub fn since_go(&self, instant: Instant) -> Duration {
        instant.saturating_duration_since(self.go_stamped)
    }

    /// The charged interval: `go` stamped to `bestmove` arrived.
    #[must_use]
    pub fn charged(&self) -> Option<Duration> {
        self.bestmove_arrived.map(|arrived| self.since_go(arrived))
    }

    /// Charged time minus the time the engine reported, in nanoseconds: what
    /// the round trip cost beyond the engine's own search. Negative when the
    /// engine reported more than it was charged, which rounding can do.
    #[must_use]
    pub fn overhead_ns(&self) -> Option<i64> {
        if self.earlier_origin {
            return None;
        }
        let charged = self.charged()?;
        let engine = self.engine_time_ms?;
        Some(signed_ns(charged) - i64::try_from(engine).ok()?.saturating_mul(1_000_000))
    }

    /// When the last `info` arrived, minus the time it reported, in
    /// nanoseconds: how far behind the engine's own clock its report reached
    /// the harness. What the engine took to start searching after the `go`
    /// was written shows up here, together with the pipe's delivery time.
    #[must_use]
    pub fn last_info_lag_ns(&self) -> Option<i64> {
        if self.earlier_origin {
            return None;
        }
        let arrived = self.since_go(self.last_info?);
        let reported = self.last_info_time_ms?;
        Some(signed_ns(arrived) - i64::try_from(reported).ok()?.saturating_mul(1_000_000))
    }
}

fn signed_ns(duration: Duration) -> i64 {
    i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn each_phase_runs_from_one_stamp_to_the_next() {
        let go = Instant::now();
        let mut timing = SearchTiming::new(go, go + ms(1), go + ms(1_000));
        timing.info(go + ms(5), Some(2));
        timing.info(go + ms(40), Some(35));
        timing.info(go + ms(41), None);
        timing.bestmove_arrived = Some(go + ms(60));
        timing.consumed = Some(go + ms(75));
        assert_eq!(timing.phase(RoundTripPhase::GoWrite), Some(ms(1)));
        assert_eq!(timing.phase(RoundTripPhase::ToFirstInfo), Some(ms(4)));
        assert_eq!(timing.phase(RoundTripPhase::BetweenInfo), Some(ms(36)));
        assert_eq!(
            timing.phase(RoundTripPhase::LastInfoToBestmove),
            Some(ms(19))
        );
        assert_eq!(
            timing.phase(RoundTripPhase::BestmoveToConsumed),
            Some(ms(15))
        );
        assert_eq!(timing.charged(), Some(ms(60)));
        // The engine's own account is its last reported time, 35 ms.
        assert_eq!(timing.engine_time_ms, Some(35));
        assert_eq!(timing.overhead_ns(), Some(25_000_000));
        // The last info carried no time, so its lag is unknown.
        assert_eq!(timing.last_info_lag_ns(), None);
    }

    #[test]
    fn a_search_with_no_answer_has_no_charged_time_or_overhead() {
        let go = Instant::now();
        let mut timing = SearchTiming::new(go, go, go + ms(50));
        timing.info(go + ms(30), Some(10));
        assert_eq!(timing.charged(), None);
        assert_eq!(timing.overhead_ns(), None);
        assert_eq!(timing.phase(RoundTripPhase::LastInfoToBestmove), None);
        assert_eq!(timing.last_info_lag_ns(), Some(20_000_000));
    }

    #[test]
    fn a_search_timed_from_ponderhit_has_no_overhead_against_a_clock_started_at_go_ponder() {
        let go = Instant::now();
        let mut timing = SearchTiming::new(go, go, go + ms(500));
        // The engine has been pondering for 400 ms and reports that.
        timing.info(go + ms(10), Some(410));
        timing.bestmove_arrived = Some(go + ms(12));
        timing.earlier_origin = true;
        assert_eq!(timing.charged(), Some(ms(12)));
        assert_eq!(timing.overhead_ns(), None);
        assert_eq!(timing.last_info_lag_ns(), None);
    }

    #[test]
    fn an_engine_reporting_more_than_it_was_charged_has_negative_overhead() {
        let go = Instant::now();
        let mut timing = SearchTiming::new(go, go, go + ms(50));
        timing.info(go + ms(10), Some(12));
        timing.bestmove_arrived = Some(go + ms(11));
        assert_eq!(timing.overhead_ns(), Some(-1_000_000));
    }
}
