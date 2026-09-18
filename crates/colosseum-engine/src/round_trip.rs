//! The per-side record of where each search's time went.
//!
//! The session stamps every step of a search ([`SearchTiming`]); this module
//! keeps what a game needs of them: per side, the largest value each phase
//! reached (stored with the game) and the stamps of the last few searches
//! (printed when the game ends in a forfeit). Both are fixed in size, so a long
//! game costs no more to record than a short one.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::time::Duration;

use colosseum_uci::{RoundTripPhase, SearchTiming};
use serde::{Deserialize, Serialize};

/// How many of a side's most recent searches a forfeit forensic prints.
pub const FORENSIC_SEARCHES: usize = 5;

/// How to read [`RoundTripRecorder::forensic`] tables.
pub const FORENSIC_LEGEND: &str = "(times are ms after `go` was stamped; `!` marks a bestmove \
that arrived after the deadline and was read only to time it; overhead = bestmove − engine t)\n";

/// The largest value each phase of one side's searches reached in a game, in
/// nanoseconds. A phase whose ends were never both observed is absent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundTripMaxima {
    /// Searches timed.
    pub searches: u32,
    /// `go` stamped → its write returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub go_write_ns: Option<u64>,
    /// Write returned → first `info` arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_first_info_ns: Option<u64>,
    /// First `info` → last `info`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub between_info_ns: Option<u64>,
    /// Last `info` → `bestmove` arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_info_to_bestmove_ns: Option<u64>,
    /// `bestmove` arrived → taken by the game task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bestmove_to_consumed_ns: Option<u64>,
    /// Last `info` arrival minus the `time` it reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_info_lag_ns: Option<i64>,
    /// Charged time minus the engine's reported time: the harness overhead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overhead_ns: Option<i64>,
}

impl RoundTripMaxima {
    fn add(&mut self, timing: &SearchTiming) {
        self.searches = self.searches.saturating_add(1);
        let phase = |phase| timing.phase(phase).map(nanos);
        raise(&mut self.go_write_ns, phase(RoundTripPhase::GoWrite));
        raise(
            &mut self.to_first_info_ns,
            phase(RoundTripPhase::ToFirstInfo),
        );
        raise(
            &mut self.between_info_ns,
            phase(RoundTripPhase::BetweenInfo),
        );
        raise(
            &mut self.last_info_to_bestmove_ns,
            phase(RoundTripPhase::LastInfoToBestmove),
        );
        raise(
            &mut self.bestmove_to_consumed_ns,
            phase(RoundTripPhase::BestmoveToConsumed),
        );
        raise(&mut self.last_info_lag_ns, timing.last_info_lag_ns());
        raise(&mut self.overhead_ns, timing.overhead_ns());
    }
}

fn raise<T: Ord + Copy>(maximum: &mut Option<T>, value: Option<T>) {
    if let Some(value) = value {
        *maximum = Some(maximum.map_or(value, |current| current.max(value)));
    }
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// One side's searches in a game: their maxima and the last few in full.
#[derive(Debug, Default)]
pub struct RoundTripRecorder {
    maxima: RoundTripMaxima,
    recent: VecDeque<(usize, SearchTiming)>,
}

impl RoundTripRecorder {
    /// Record the search for half-move `ply` (counting from one). A search
    /// that missed its deadline is recorded with whatever was learned of its
    /// `bestmove` afterwards: it is the search a forfeit is about.
    pub fn record(&mut self, ply: usize, timing: SearchTiming) {
        self.maxima.add(&timing);
        if self.recent.len() == FORENSIC_SEARCHES {
            self.recent.pop_front();
        }
        self.recent.push_back((ply, timing));
    }

    /// The maxima, or `None` when this side never searched.
    #[must_use]
    pub fn maxima(&self) -> Option<RoundTripMaxima> {
        (self.maxima.searches > 0).then(|| self.maxima.clone())
    }

    /// The last searches as a table, milliseconds from each `go` stamp. Read
    /// it with [`FORENSIC_LEGEND`].
    #[must_use]
    pub fn forensic(&self, side: &str) -> String {
        let mut text = String::new();
        if self.recent.is_empty() {
            let _ = writeln!(text, "\n── {side} round trip: no searches ──");
            return text;
        }
        let _ = writeln!(
            text,
            "\n── {side} round trip, last {} searches ──",
            self.recent.len()
        );
        let _ = writeln!(
            text,
            "{:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "ply",
            "written",
            "1st info",
            "lst info",
            "engine t",
            "bestmove",
            "consumed",
            "overhead",
            "deadline"
        );
        for (ply, timing) in &self.recent {
            let at = |instant: Option<std::time::Instant>| {
                instant.map_or_else(|| "-".to_owned(), |instant| ms(timing.since_go(instant)))
            };
            let bestmove = match timing.bestmove_arrived {
                Some(arrived) if timing.late => format!("{}!", ms(timing.since_go(arrived))),
                Some(arrived) => ms(timing.since_go(arrived)),
                None => "never".to_owned(),
            };
            let _ = writeln!(
                text,
                "{:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
                ply,
                ms(timing.since_go(timing.write_returned)),
                at(timing.first_info),
                at(timing.last_info),
                timing
                    .engine_time_ms
                    .map_or_else(|| "-".to_owned(), |value| value.to_string()),
                bestmove,
                at(timing.consumed),
                timing
                    .overhead_ns()
                    .map_or_else(|| "-".to_owned(), signed_ms),
                ms(timing.since_go(timing.deadline)),
            );
        }
        text
    }
}

fn ms(duration: Duration) -> String {
    format!("{:.1}", duration.as_secs_f64() * 1_000.0)
}

fn signed_ms(nanos: i64) -> String {
    format!("{:.1}", nanos as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn timing(go: Instant, bestmove_ms: u64, engine_ms: u64) -> SearchTiming {
        let mut timing = SearchTiming::new(
            go,
            go + Duration::from_millis(1),
            go + Duration::from_millis(500),
        );
        timing.info(go + Duration::from_millis(3), Some(1));
        timing.info(go + Duration::from_millis(bestmove_ms - 1), Some(engine_ms));
        timing.bestmove_arrived = Some(go + Duration::from_millis(bestmove_ms));
        timing.consumed = Some(go + Duration::from_millis(bestmove_ms + 2));
        timing
    }

    #[test]
    fn maxima_keep_the_largest_value_of_each_phase_and_the_ring_the_last_five() {
        let go = Instant::now();
        let mut recorder = RoundTripRecorder::default();
        for (index, (bestmove, engine)) in
            [(20, 15), (80, 30), (40, 38), (30, 25), (25, 20), (22, 21)]
                .into_iter()
                .enumerate()
        {
            recorder.record(index * 2 + 1, timing(go, bestmove, engine));
        }
        let maxima = recorder.maxima().unwrap();
        assert_eq!(maxima.searches, 6);
        assert_eq!(maxima.go_write_ns, Some(1_000_000));
        assert_eq!(maxima.to_first_info_ns, Some(2_000_000));
        assert_eq!(maxima.between_info_ns, Some(76_000_000));
        assert_eq!(maxima.last_info_to_bestmove_ns, Some(1_000_000));
        assert_eq!(maxima.bestmove_to_consumed_ns, Some(2_000_000));
        assert_eq!(maxima.overhead_ns, Some(50_000_000));
        assert_eq!(maxima.last_info_lag_ns, Some(49_000_000));
        let table = recorder.forensic("white");
        assert!(table.contains("last 5 searches"), "{table}");
        assert!(
            !table.contains("\n    1 "),
            "the first search left the ring:\n{table}"
        );
        assert!(table.contains("\n   11 "), "{table}");
    }

    #[test]
    fn a_search_that_never_answered_is_printed_as_never() {
        let go = Instant::now();
        let mut recorder = RoundTripRecorder::default();
        let mut unanswered = SearchTiming::new(go, go, go + Duration::from_millis(50));
        unanswered.info(go + Duration::from_millis(10), Some(8));
        recorder.record(7, unanswered);
        assert!(
            RoundTripRecorder::default()
                .forensic("black")
                .contains("no searches")
        );
        let table = recorder.forensic("black");
        assert!(table.contains("never"), "{table}");
        let mut late = unanswered;
        late.bestmove_arrived = Some(go + Duration::from_millis(320));
        late.late = true;
        let mut recorder = RoundTripRecorder::default();
        recorder.record(7, late);
        assert!(recorder.forensic("black").contains("320.0!"));
        let maxima = recorder.maxima().unwrap();
        assert_eq!(maxima.searches, 1);
        assert_eq!(maxima.overhead_ns, Some(312_000_000));
        assert_eq!(maxima.last_info_to_bestmove_ns, Some(310_000_000));
    }
}
