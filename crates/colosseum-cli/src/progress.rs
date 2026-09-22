//! Progress blocks: what a long run tells the operator, and when.
//!
//! A progress report is counted in the run's own unit — pairs, games or
//! iterations — because that is what the operator is waiting for and what a
//! decision is made of. A wall-clock schedule reports a different amount of
//! evidence every time and tells a fast run the same thing as a slow one.
//!
//! A time floor is still needed in one direction only: a fixed-node run can
//! finish a unit in milliseconds, and printing every boundary would bury the
//! console. The floor coalesces those blocks and never delays one past the
//! next boundary once it has elapsed.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The unit a command counts its own progress in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressUnit {
    /// Colour-reversed pairs: the atomic statistical unit.
    Pairs,
    /// Individual games, for a command whose unit is not a pair.
    Games,
    /// SPSA iterations, each one complete mini-match.
    Iterations,
}

impl ProgressUnit {
    #[must_use]
    pub fn plural(self) -> &'static str {
        match self {
            Self::Pairs => "pairs",
            Self::Games => "games",
            Self::Iterations => "iterations",
        }
    }
}

/// One labelled line of a block, in display order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressField {
    pub label: String,
    pub value: String,
}

/// One progress report: the same content on standard error, in `run.log` and
/// from `status`, so a console and an audit never disagree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressBlock {
    pub command: String,
    pub unit: ProgressUnit,
    /// Units completed and counted.
    pub done: u64,
    /// Units the run is allowed to reach, where the command has a cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    pub elapsed_seconds: f64,
    pub fields: Vec<ProgressField>,
    /// Lines the block records but does not print.
    ///
    /// A console has to stay readable, and some of what a run knows is only
    /// useful afterwards: one iteration's mini-match, the gain and
    /// perturbation scale it used, what moved since the previous block. The
    /// trajectory belongs in `run.log` and in the command's own status view,
    /// not in every block on the terminal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<ProgressField>,
}

impl ProgressBlock {
    #[must_use]
    pub fn new(
        command: impl Into<String>,
        unit: ProgressUnit,
        done: u64,
        total: Option<u64>,
        elapsed: Duration,
    ) -> Self {
        Self {
            command: command.into(),
            unit,
            done,
            total,
            elapsed_seconds: elapsed.as_secs_f64(),
            fields: Vec::new(),
            detail: Vec::new(),
        }
    }

    /// Add one labelled line to the printed block.
    pub fn field(&mut self, label: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.fields.push(ProgressField {
            label: label.into(),
            value: value.into(),
        });
        self
    }

    /// Add one labelled line that is recorded but not printed.
    pub fn detail(&mut self, label: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.detail.push(ProgressField {
            label: label.into(),
            value: value.into(),
        });
        self
    }

    /// The headline: where the run stands in its own unit.
    #[must_use]
    pub fn headline(&self) -> String {
        let unit = self.unit.plural();
        let elapsed = format_duration(self.elapsed_seconds);
        match self.total {
            Some(total) if total > 0 => format!(
                "{}/{total} {unit} ({:.0}%), {elapsed} elapsed",
                self.done,
                100.0 * self.done as f64 / total as f64
            ),
            _ => format!("{} {unit}, {elapsed} elapsed", self.done),
        }
    }

    /// The block as an operator reads it, newline-terminated.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = format!("progress [{}]: {}\n", self.command, self.headline());
        let width = self
            .fields
            .iter()
            .map(|field| field.label.chars().count())
            .max()
            .unwrap_or(0);
        for field in &self.fields {
            let padding = " ".repeat(width - field.label.chars().count());
            let _ = writeln!(text, "  {}{padding}  {}", field.label, field.value);
        }
        text
    }

    /// The same block as one `run.log` event.
    #[must_use]
    pub fn log_event(&self) -> Value {
        json!({
            "event": "progress",
            "progress": self,
        })
    }
}

/// Seconds as an operator reads them: `45s`, `6m30s`, `2h05m`.
#[must_use]
pub fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unknown".to_owned();
    }
    let total = seconds.round() as u64;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

/// Decides when the next block is due.
///
/// Due means "at least `every` units since the last block, and at least
/// `floor` since it was printed". The unit condition is what schedules a
/// block; the floor only ever withholds one, and withholds it no longer than
/// until the next poll after it expires.
#[derive(Debug)]
pub struct ProgressSchedule {
    every: u64,
    floor: Duration,
    started: Instant,
    last_at: Instant,
    last_units: u64,
    /// Units of the last block actually published, so a final block that would
    /// repeat one is not printed twice.
    published: Option<u64>,
    /// Units this invocation inherited from a resumed run. They are progress,
    /// but this invocation did not spend time on them, so no rate may claim
    /// them.
    resumed: u64,
}

impl ProgressSchedule {
    /// `every` is in the command's own unit and `min_secs` is the floor, which
    /// is at least one second so that it, and never the polling a run does to
    /// notice a boundary, is what decides how close two blocks may be.
    /// `resumed_units` are units this run already had before it started, so a
    /// resumed run does not print a block for work it did not just do.
    #[must_use]
    pub fn new(every: u64, min_secs: u64, resumed_units: u64) -> Self {
        Self::started_at(every, min_secs, resumed_units, Instant::now())
    }

    /// The same schedule, started at `started` rather than now, so the floor
    /// can be driven by instants a caller chooses.
    #[must_use]
    pub fn started_at(every: u64, min_secs: u64, resumed_units: u64, started: Instant) -> Self {
        Self {
            every: every.max(1),
            floor: Duration::from_secs(min_secs),
            started,
            last_at: started,
            last_units: resumed_units,
            published: None,
            resumed: resumed_units,
        }
    }

    /// Units completed since this invocation started.
    #[must_use]
    pub fn units_since_start(&self, done: u64) -> u64 {
        done.saturating_sub(self.resumed)
    }

    /// True when `done` units mean a block is due now. Records the emission,
    /// so a caller that asks is a caller that prints.
    pub fn due(&mut self, done: u64) -> bool {
        self.due_at(done, Instant::now())
    }

    /// [`Self::due`], asked at `now` instead of the current instant.
    pub fn due_at(&mut self, done: u64, now: Instant) -> bool {
        if done < self.last_units.saturating_add(self.every)
            || now.duration_since(self.last_at) < self.floor
        {
            return false;
        }
        self.last_at = now;
        self.last_units = done;
        self.published = Some(done);
        true
    }

    /// True when a final block at termination would say something the last
    /// published block did not.
    #[must_use]
    pub fn needs_final(&self, done: u64) -> bool {
        self.published != Some(done)
    }

    /// Record a block the caller printed for its own reason, such as the final
    /// one at termination.
    pub fn mark(&mut self, done: u64) {
        self.last_at = Instant::now();
        self.last_units = done;
        self.published = Some(done);
    }

    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// How long this invocation has been running, in hours, for a rate.
    #[must_use]
    pub fn elapsed_hours(&self) -> f64 {
        self.started.elapsed().as_secs_f64() / 3600.0
    }
}

/// Units per hour, or `None` before any time or work has passed.
#[must_use]
pub fn rate_per_hour(units: u64, hours: f64) -> Option<f64> {
    (hours > 0.0 && units > 0).then(|| units as f64 / hours)
}

/// A signed value with an explicit sign, as an Elo estimate is read.
#[must_use]
pub fn signed(value: f64) -> String {
    // A negative zero is arithmetic, not a measurement, and reads as a claim.
    let value = if value == 0.0 { 0.0 } else { value };
    format!("{value:+.1}")
}

/// A point estimate with the half-width of its 95% interval.
///
/// An operator reads an Elo estimate as "this much, give or take that much",
/// so the margin is what the block shows. The bounds are the same fact and
/// are what the final report and the JSON carry.
#[must_use]
pub fn estimate(point: f64, lower: f64, upper: f64) -> String {
    let margin = (upper - lower) / 2.0;
    format!("{} +/- {margin:.1}", signed(point))
}

/// A count with its noun, so a block never reports "1 pairs".
#[must_use]
pub fn plural(count: u64, singular: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {singular}s")
    }
}

/// The rule between two blocks, so a long console reads as a sequence of
/// reports rather than one wall of lines.
pub const BLOCK_SEPARATOR: &str = "--------------------------------------------------";

/// How long `remaining` more units take at the rate observed so far.
///
/// `done` counts only the units this invocation played, because a resumed run
/// did not spend time on the ones it inherited.
#[must_use]
pub fn time_for_units(done: u64, elapsed: Duration, remaining: u64) -> Option<Duration> {
    if done == 0 {
        return None;
    }
    if remaining == 0 {
        return Some(Duration::ZERO);
    }
    let per_unit = elapsed.as_secs_f64() / done as f64;
    let left = per_unit * remaining as f64;
    left.is_finite()
        .then(|| Duration::from_secs_f64(left.max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_due_on_the_unit_boundary_and_not_before() {
        let mut schedule = ProgressSchedule::new(10, 0, 0);
        assert!(!schedule.due(9));
        assert!(schedule.due(10));
        // The next boundary is ten units after the block that was printed,
        // not after the boundary it was scheduled on.
        assert!(!schedule.due(19));
        assert!(schedule.due(21));
    }

    #[test]
    fn a_resumed_run_counts_from_the_units_it_inherited() {
        let mut schedule = ProgressSchedule::new(10, 0, 40);
        assert!(!schedule.due(45));
        assert!(schedule.due(50));
    }

    #[test]
    fn the_floor_withholds_a_block_and_then_releases_it() {
        let mut schedule = ProgressSchedule::new(1, 3600, 0);
        // The boundary is long past, but the floor has not elapsed.
        assert!(!schedule.due(50));
        schedule.floor = Duration::ZERO;
        // The withheld block is not skipped: the next poll prints it.
        assert!(schedule.due(50));
    }

    #[test]
    fn the_floor_coalesces_boundaries_until_it_has_elapsed() {
        let started = Instant::now();
        let at = |millis| started + Duration::from_millis(millis);
        let mut schedule = ProgressSchedule::started_at(2, 1, 0, started);
        // A boundary inside the floor is withheld, and the next poll after
        // the floor expires prints it.
        assert!(!schedule.due_at(2, at(500)));
        assert!(schedule.due_at(2, at(1_000)));
        // The floor counts from the block just printed: two more boundaries
        // inside it are one block once it has elapsed, not two.
        assert!(!schedule.due_at(4, at(1_500)));
        assert!(!schedule.due_at(6, at(1_900)));
        assert!(schedule.due_at(6, at(2_000)));
        assert!(!schedule.due_at(7, at(60_000)));
    }

    #[test]
    fn elapsed_time_alone_never_makes_a_block_due() {
        let started = Instant::now();
        let mut schedule = ProgressSchedule::started_at(1_000, 1, 0, started);
        let much_later = started + Duration::from_secs(10 * 3600);
        assert!(!schedule.due_at(0, much_later));
        assert!(!schedule.due_at(999, much_later));
        assert!(schedule.due_at(1_000, much_later));
    }

    #[test]
    fn a_headline_reads_as_a_fraction_of_the_cap() {
        let block = ProgressBlock::new(
            "sprt",
            ProgressUnit::Pairs,
            42,
            Some(200),
            Duration::from_secs(390),
        );
        assert_eq!(block.headline(), "42/200 pairs (21%), 6m30s elapsed");
        let uncapped = ProgressBlock::new(
            "match",
            ProgressUnit::Games,
            7,
            None,
            Duration::from_secs(9),
        );
        assert_eq!(uncapped.headline(), "7 games, 9s elapsed");
    }

    #[test]
    fn a_rendered_block_aligns_its_labels() {
        let mut block = ProgressBlock::new(
            "match",
            ProgressUnit::Games,
            4,
            Some(8),
            Duration::from_secs(60),
        );
        block.field("score", "2.5/4").field("W/D/L", "2/1/1");
        assert_eq!(
            block.render(),
            "progress [match]: 4/8 games (50%), 1m00s elapsed\n  \
             score  2.5/4\n  W/D/L  2/1/1\n"
        );
    }

    #[test]
    fn a_final_block_is_not_a_repeat_of_the_last_one() {
        let mut schedule = ProgressSchedule::new(10, 0, 0);
        assert!(schedule.needs_final(0), "nothing has been published");
        assert!(schedule.due(10));
        assert!(!schedule.needs_final(10), "the boundary block said this");
        assert!(schedule.needs_final(11));
    }

    #[test]
    fn a_rate_never_claims_the_work_a_resume_inherited() {
        let schedule = ProgressSchedule::new(10, 5, 40);
        assert_eq!(schedule.units_since_start(52), 12);
    }

    #[test]
    fn a_detail_line_is_recorded_but_never_printed() {
        let mut block = ProgressBlock::new(
            "spsa",
            ProgressUnit::Iterations,
            2,
            Some(4),
            Duration::from_secs(1),
        );
        block
            .field("faults", "engine 0/0")
            .detail("gain", "a 0.0031");
        assert_eq!(
            block.render(),
            "progress [spsa]: 2/4 iterations (50%), 1s elapsed\n  faults  engine 0/0\n"
        );
        assert_eq!(block.log_event()["progress"]["detail"][0]["label"], "gain");
    }

    #[test]
    fn a_count_agrees_with_its_noun() {
        assert_eq!(plural(1, "pair"), "1 pair");
        assert_eq!(plural(0, "pair"), "0 pairs");
        assert_eq!(plural(42, "pair"), "42 pairs");
    }

    #[test]
    fn a_negative_zero_is_never_reported_as_one() {
        assert_eq!(signed(-0.0), "+0.0");
        assert_eq!(estimate(-0.0, -0.0, 0.0), "+0.0 +/- 0.0");
    }

    #[test]
    fn an_estimate_reads_as_a_value_and_a_margin() {
        assert_eq!(estimate(35.72, 18.2, 53.24), "+35.7 +/- 17.5");
        assert_eq!(estimate(-4.0, -20.0, 12.0), "-4.0 +/- 16.0");
    }

    #[test]
    fn a_finished_run_has_no_time_left_and_an_idle_one_has_no_estimate() {
        assert_eq!(
            time_for_units(4, Duration::from_secs(40), 0),
            Some(Duration::ZERO)
        );
        assert_eq!(
            time_for_units(4, Duration::from_secs(40), 6),
            Some(Duration::from_secs(60))
        );
        assert_eq!(time_for_units(0, Duration::from_secs(40), 6), None);
    }

    #[test]
    fn durations_read_the_way_an_operator_reads_them() {
        assert_eq!(format_duration(9.0), "9s");
        assert_eq!(format_duration(90.0), "1m30s");
        assert_eq!(format_duration(7500.0), "2h05m");
        assert_eq!(format_duration(f64::NAN), "unknown");
    }
}
