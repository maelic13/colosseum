//! Resolvers and output shared by more than one command.

use super::*;

#[derive(Debug, Args)]
pub(crate) struct MatchConditions {
    #[arg(long = "a-label")]
    pub(crate) a_label: Option<String>,
    #[arg(long = "a-engine-arg", allow_hyphen_values = true)]
    pub(crate) a_arguments: Vec<OsString>,
    #[arg(long = "a-cwd")]
    pub(crate) a_cwd: Option<PathBuf>,
    #[arg(long = "a-env", value_name = "KEY=VALUE")]
    pub(crate) a_environment: Vec<String>,
    #[arg(long = "a-option", value_name = "NAME=VALUE")]
    pub(crate) a_options: Vec<String>,
    #[arg(long = "a-button", value_name = "NAME")]
    pub(crate) a_buttons: Vec<String>,
    #[arg(long = "a-cores", value_name = "LIST")]
    pub(crate) a_cores: Option<String>,
    #[arg(long = "a-movetime-ms", value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) a_movetime_ms: Option<u64>,
    #[arg(long = "a-base-ms", value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) a_base_ms: Option<u64>,
    #[arg(long = "a-increment-ms")]
    pub(crate) a_increment_ms: Option<u64>,
    #[arg(long = "a-nodes", value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) a_nodes: Option<u64>,
    #[arg(long = "a-depth", value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) a_depth: Option<u32>,
    #[arg(long = "a-margin-ms", default_value_t = match_runner::DEFAULT_MARGIN_MS)]
    pub(crate) a_margin_ms: u64,

    #[arg(long = "b-label")]
    pub(crate) b_label: Option<String>,
    #[arg(long = "b-engine-arg", allow_hyphen_values = true)]
    pub(crate) b_arguments: Vec<OsString>,
    #[arg(long = "b-cwd")]
    pub(crate) b_cwd: Option<PathBuf>,
    #[arg(long = "b-env", value_name = "KEY=VALUE")]
    pub(crate) b_environment: Vec<String>,
    #[arg(long = "b-option", value_name = "NAME=VALUE")]
    pub(crate) b_options: Vec<String>,
    #[arg(long = "b-button", value_name = "NAME")]
    pub(crate) b_buttons: Vec<String>,
    #[arg(long = "b-cores", value_name = "LIST")]
    pub(crate) b_cores: Option<String>,
    #[arg(long = "b-movetime-ms", value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) b_movetime_ms: Option<u64>,
    #[arg(long = "b-base-ms", value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) b_base_ms: Option<u64>,
    #[arg(long = "b-increment-ms")]
    pub(crate) b_increment_ms: Option<u64>,
    #[arg(long = "b-nodes", value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) b_nodes: Option<u64>,
    #[arg(long = "b-depth", value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) b_depth: Option<u32>,
    #[arg(long = "b-margin-ms", default_value_t = match_runner::DEFAULT_MARGIN_MS)]
    pub(crate) b_margin_ms: u64,

    /// Let engines think on the opponent's clock through the UCI ponder protocol.
    /// Both engines then search at once, so each needs its own cores.
    #[arg(long, requires = "cores_per_engine")]
    pub(crate) ponder: bool,

    /// Whether a slot keeps its two engine processes from game to game
    /// (`per-slot`, restarting one after a fault) or starts fresh ones for
    /// every game (`per-game`).
    #[arg(long, value_enum, default_value = "per-slot")]
    pub(crate) engine_processes: match_runner::EngineProcesses,

    /// Adjudicate a draw once both engines agree; off unless requested.
    #[arg(long)]
    pub(crate) draw_adjudication: bool,
    #[arg(long, default_value_t = 40, value_parser = clap::value_parser!(u32).range(1..), requires = "draw_adjudication")]
    pub(crate) draw_move: u32,
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..), requires = "draw_adjudication")]
    pub(crate) draw_moves: u32,
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(i32).range(0..), requires = "draw_adjudication")]
    pub(crate) draw_score_cp: i32,

    /// Adjudicate a resignation once both engines agree; off unless requested.
    #[arg(long)]
    pub(crate) resign_adjudication: bool,
    /// Use only the losing engine's evaluations for resignation adjudication.
    #[arg(long, requires = "resign_adjudication")]
    pub(crate) one_sided_resign_adjudication: bool,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..), requires = "resign_adjudication")]
    pub(crate) resign_moves: u32,
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(i32).range(1..), requires = "resign_adjudication")]
    pub(crate) resign_score_cp: i32,

    /// Draw after this many full moves; omitted means no maximum-move cap.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) max_moves: Option<u32>,

    /// Invalidate after more engine faults than this; a time loss is one.
    /// Omitted: 1% of the scheduled games, at least 5, for `match` and
    /// `calibrate`; for `sprt`, 0.5% of the games played so far, at least 3.
    #[arg(long)]
    pub(crate) max_engine_faults: Option<u32>,
    /// Invalidate after more time losses than this. Omitted: the engine-fault
    /// allowance, which already counts them.
    #[arg(long)]
    pub(crate) max_time_losses: Option<u32>,

    /// Number of games allowed to run at once.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) concurrency: u32,
    /// Physical cores per game slot, shared by both of its engines. This is
    /// the default allocation, because without pondering only one engine of a
    /// game searches at a time.
    #[arg(
        long,
        value_parser = clap::value_parser!(u32).range(1..),
        conflicts_with = "cores_per_engine"
    )]
    pub(crate) cores_per_game: Option<u32>,
    /// Physical cores allocated separately to each engine in each game slot.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) cores_per_engine: Option<u32>,
    /// CPU placement: off, auto, or an explicit logical CPU list.
    #[arg(long, default_value = "off")]
    pub(crate) placement: String,
    /// Whole physical cores left free when placement is auto.
    /// Whole physical cores left free for the harness and the operating system.
    #[arg(long, default_value_t = colosseum_engine::DEFAULT_AUTO_HEADROOM_PHYSICAL_CORES)]
    pub(crate) headroom_cores: usize,
    /// Trusted hard budget for the two engines' configured Hash memory.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) memory_budget_mb: Option<u64>,

    /// Optional EPD or PGN opening book; format is inferred from the extension.
    #[arg(long)]
    pub(crate) book: Option<PathBuf>,
    /// Opening order within the book.
    #[arg(long, value_enum, default_value_t = BookOrderArg::Sequential)]
    pub(crate) book_order: BookOrderArg,
    /// Zero-based first opening after ordering.
    #[arg(long, default_value_t = 0)]
    pub(crate) book_start: usize,
    /// Reuse openings from the start of the book once they run out, instead of
    /// refusing a schedule the book cannot cover.
    #[arg(long, requires = "book")]
    pub(crate) book_wrap: bool,
    /// PGN half-moves to pre-play; EPD positions ignore this value.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) book_plies: Option<u32>,
    /// Master seed for every random choice; generated and reported when omitted.
    #[arg(long)]
    pub(crate) seed: Option<u64>,

    /// Self-contained run directory; an existing matching directory resumes.
    #[arg(long = "dir")]
    pub(crate) run_directory: Option<PathBuf>,
    /// Archive an existing --dir and start a fresh run there.
    #[arg(long, requires = "run_directory")]
    pub(crate) restart: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum BookOrderArg {
    Sequential,
    Random,
}

impl MatchConditions {
    /// The fault policy these flags describe, given the command's own default
    /// engine-fault allowance. Time losses are engine faults, so an omitted
    /// time-loss limit is the engine-fault allowance itself.
    pub(crate) fn fault_policy(&self, default_engine_faults: u32) -> FaultPolicy {
        let max_engine_faults = self.max_engine_faults.unwrap_or(default_engine_faults);
        FaultPolicy {
            max_engine_faults,
            max_time_losses: self.max_time_losses.unwrap_or(max_engine_faults),
            rate: None,
        }
    }

    /// The fault policy of a sequential test: see [`FaultPolicy::sequential`].
    pub(crate) fn sequential_fault_policy(&self) -> FaultPolicy {
        FaultPolicy::sequential(self.max_engine_faults, self.max_time_losses)
    }
}

/// The fewest forfeits a fixed-size run tolerates by default.
pub(crate) const FORFEIT_ALLOWANCE_MINIMUM: u32 = 5;

/// The engine faults a fixed-size run tolerates by default: 1% of its
/// scheduled games, and never fewer than [`FORFEIT_ALLOWANCE_MINIMUM`].
///
/// A long run on a busy host meets the occasional scheduling stall however
/// carefully it is placed, and a single forfeit in 30,000 games says nothing
/// about either engine; a stable one-in-a-hundred rate says something is
/// wrong. `sprt` and `spsa` do not know their length in advance, so their
/// allowance grows with the games played instead; see
/// [`FaultPolicy::sequential`].
pub(crate) fn forfeit_allowance(scheduled_games: u64) -> u32 {
    u32::try_from(scheduled_games / 100)
        .unwrap_or(u32::MAX)
        .max(FORFEIT_ALLOWANCE_MINIMUM)
}

/// The two sides of a paired run, by the names a reader knows them by.
#[derive(Debug, Clone)]
pub(crate) struct Players {
    pub(crate) a: String,
    pub(crate) b: String,
}

impl Players {
    pub(crate) fn new(a: &EngineLaunchSpec, b: &EngineLaunchSpec) -> Self {
        Self {
            a: engine_display_name(a),
            b: engine_display_name(b),
        }
    }
}

impl std::fmt::Display for Players {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} vs. {}", self.a, self.b)
    }
}

/// Faults per side, engine A's first, as `time: 0-2; other: 0-0`: a `0/2`
/// read as "none of two" rather than "none for A, two for B".
pub(crate) fn fault_counts_text(faults: MatchFaultCounts) -> String {
    format!(
        "time: {}-{}; other: {}-{}",
        faults.time_losses_a,
        faults.time_losses_b,
        faults.engine_a.saturating_sub(faults.time_losses_a),
        faults.engine_b.saturating_sub(faults.time_losses_b),
    )
}

/// How a progress block and a final report state the faults so far: their
/// rate over the games played and the allowance at that point.
pub(crate) fn fault_allowance_text(
    policy: FaultPolicy,
    faults: MatchFaultCounts,
    games: u64,
) -> String {
    // One short clause: the rule behind the allowance is documented, and the
    // per-side counts already stand beside it on the same line.
    let engine = u64::from(faults.engine_total());
    let engine_limit = policy.engine_limit(games);
    let time_limit = policy.time_limit(games);
    if time_limit < engine_limit {
        format!("{engine} of {engine_limit} allowed, time losses max {time_limit}")
    } else {
        format!("{engine} of {engine_limit} allowed")
    }
}

pub(crate) fn match_engine_overrides_requested(conditions: &MatchConditions) -> bool {
    conditions.a_label.is_some()
        || !conditions.a_arguments.is_empty()
        || conditions.a_cwd.is_some()
        || !conditions.a_environment.is_empty()
        || !conditions.a_options.is_empty()
        || !conditions.a_buttons.is_empty()
        || conditions.a_cores.is_some()
        || conditions.b_label.is_some()
        || !conditions.b_arguments.is_empty()
        || conditions.b_cwd.is_some()
        || !conditions.b_environment.is_empty()
        || !conditions.b_options.is_empty()
        || !conditions.b_buttons.is_empty()
        || conditions.b_cores.is_some()
}

pub(crate) fn parse_positive_seconds(value: &str) -> Result<f64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("expected a finite positive number of seconds, got {value}"))?;
    if parsed.is_finite() && parsed > 0.0 {
        Ok(parsed)
    } else {
        Err(format!(
            "expected a finite positive number of seconds, got {value}"
        ))
    }
}

pub(crate) fn parse_indexed_values(
    values: &[String],
    engine_count: usize,
    option: &str,
) -> Result<Vec<Vec<String>>, String> {
    let mut grouped = vec![Vec::new(); engine_count];
    for value in values {
        let Some((index, payload)) = value.split_once(':') else {
            return Err(format!("{option} must use INDEX:VALUE syntax"));
        };
        let index = index
            .parse::<usize>()
            .ok()
            .filter(|index| (1..=engine_count).contains(index))
            .ok_or_else(|| format!("{option} index is outside the --engine list: {index}"))?;
        if payload.is_empty() {
            return Err(format!("{option} value must not be empty"));
        }
        grouped[index - 1].push(payload.to_owned());
    }
    Ok(grouped)
}

pub(crate) fn resolve_invocation(
    action: &str,
    engine: &EngineArgs,
) -> Result<(EngineLaunchSpec, crate::ResolvedConfig), Box<dyn std::error::Error>> {
    let launch = engine.resolve()?;
    let mut path_pointers = vec!["/engine/executable".to_owned()];
    if launch.working_directory.is_some() {
        path_pointers.push("/engine/working_directory".to_owned());
    }
    let current_directory = std::env::current_dir()?;
    let resolved = resolve_config(
        built_in_defaults(),
        None,
        json!({ "command": action, "engine": launch }),
        &[],
        Path::new(&current_directory),
        &path_pointers,
    )?;
    let launch = serde_json::from_value(
        resolved
            .value()
            .get("engine")
            .cloned()
            .expect("engine is part of the CLI layer"),
    )?;
    Ok((launch, resolved))
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum MachineOutput<'a> {
    Capabilities {
        report: capabilities::CapabilitiesReport,
    },
    FixedMatch {
        run_directory: PathBuf,
        report: match_runner::FixedMatchReport,
    },
    Sprt {
        run_directory: PathBuf,
        report: sprt_runner::SprtReport,
    },
    Spsa {
        run_directory: PathBuf,
        report: Box<SpsaReport>,
    },
    SpsaPlan {
        report: SpsaPlanReport,
    },
    SpsaStatus {
        run_directory: &'a Path,
        report: SpsaStatusOutput,
    },
    Calibration {
        run_directory: PathBuf,
        report: CalibrationReport,
    },
    DryRun {
        command: &'a str,
        config_sha256: &'a str,
        resolved_configuration: &'a Value,
        invocations: Vec<&'a EngineLaunchSpec>,
        /// For `spsa`: how an iteration's games would fill the slots.
        #[serde(skip_serializing_if = "Option::is_none")]
        wave_shape: Option<SpsaWaveShape>,
    },
    EngineInspection {
        inspection: EngineInspection,
    },
    EngineCompliance {
        report: ComplianceReport,
    },
    BookHash {
        report: BookHashReport,
    },
    BookStats {
        report: BookStatsReport,
    },
    BookVerify {
        audit: colosseum_engine::OpeningAudit,
    },
    BookSlice {
        report: BookSliceReport,
    },
    StatsReplay {
        report: crate::stats_replay::StatsReplayReport,
    },
    StatsFixedPlan {
        report: FixedPlanReport,
    },
    StatsSprtPlan {
        report: SprtLengthPlanReport,
    },
    Nps {
        report: NpsReport,
    },
    NpsComparison {
        report: NpsExperimentReport,
    },
    NpsScaling {
        report: NpsScalingReport,
    },
    TournamentPlan {
        report: TournamentPlan,
    },
    Tournament {
        run_directory: PathBuf,
        report: tournament_driver::TournamentReport,
    },
    SelfTest {
        report: self_test::SelfTestReport,
    },
    RunStatus {
        run_directory: &'a Path,
        record: RunRecord,
        /// The checkpoint's aggregates and the journal after it, for a run
        /// that keeps them.
        #[serde(skip_serializing_if = "Option::is_none")]
        durable: Option<Value>,
    },
}

pub(crate) fn executable_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("cannot hash executable {}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(crate) fn read_stored_seed(root: &Path) -> Option<(u64, bool)> {
    let bytes = fs::read(root.join("resolved-config.json")).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    Some((
        value.get("master_seed")?.as_u64()?,
        value
            .get("master_seed_generated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    ))
}

pub(crate) fn resolve_master_seed(configured: Option<u64>) -> Result<(u64, bool), String> {
    if let Some(seed) = configured {
        return Ok((seed, false));
    }
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok((u64::from_le_bytes(bytes), true))
}

pub(crate) fn resolve_placement(
    value: &str,
    headroom_cores: usize,
) -> Result<CpuPlacementPolicy, String> {
    match value {
        "off" => Ok(CpuPlacementPolicy::Off),
        "auto" => Ok(CpuPlacementPolicy::Auto {
            headroom_physical_cores: headroom_cores,
        }),
        explicit => parse_cpu_list(explicit)
            .map(|cpus| CpuPlacementPolicy::Explicit { cpus })
            .map_err(|error| error.to_string()),
    }
}

pub(crate) fn resolve_adjudication(command: &MatchConditions) -> AdjudicationConfig {
    AdjudicationConfig {
        max_moves: command.max_moves,
        draw: command.draw_adjudication.then_some(DrawAdjudication {
            min_ply: command.draw_move.saturating_mul(2),
            move_count: command.draw_moves,
            score_cp: command.draw_score_cp,
        }),
        resign: command.resign_adjudication.then_some(ResignAdjudication {
            move_count: command.resign_moves,
            score_cp: command.resign_score_cp,
            two_sided: !command.one_sided_resign_adjudication,
        }),
    }
}

pub(crate) fn resolve_time_control(
    side: &str,
    movetime_ms: Option<u64>,
    base_ms: Option<u64>,
    increment_ms: Option<u64>,
    nodes: Option<u64>,
    depth: Option<u32>,
    margin_ms: u64,
) -> Result<match_runner::ConfiguredTimeControl, String> {
    let selected = usize::from(movetime_ms.is_some())
        + usize::from(base_ms.is_some())
        + usize::from(nodes.is_some())
        + usize::from(depth.is_some());
    if selected > 1 {
        return Err(format!(
            "{side} must select only one of movetime, base/increment, nodes or depth"
        ));
    }
    if increment_ms.is_some() && base_ms.is_none() {
        return Err(format!("{side} increment requires a base time"));
    }
    let control = if let Some(ms) = movetime_ms {
        TimeControl::PerMove { ms }
    } else if let Some(base_ms) = base_ms {
        match increment_ms {
            Some(inc_ms) => TimeControl::Increment { base_ms, inc_ms },
            None => TimeControl::SuddenDeath { base_ms },
        }
    } else if let Some(nodes) = nodes {
        TimeControl::Nodes { nodes }
    } else if let Some(depth) = depth {
        TimeControl::Depth { depth }
    } else {
        TimeControl::Increment {
            base_ms: match_runner::DEFAULT_BASE_MS,
            inc_ms: match_runner::DEFAULT_INCREMENT_MS,
        }
    };
    Ok(match_runner::ConfiguredTimeControl { control, margin_ms })
}

pub(crate) fn validate_ponder(
    ponder: bool,
    controls: &[match_runner::ConfiguredTimeControl],
) -> Result<(), String> {
    if ponder && controls.iter().any(|control| !control.control.is_clock()) {
        return Err(
            "--ponder requires a base/increment clock on every arm; fixed movetime, nodes and depth have no opponent-clock budget"
                .into(),
        );
    }
    Ok(())
}

pub(crate) fn configure_ponder(engine: &mut EngineLaunchSpec, ponder: bool) -> Result<(), String> {
    if engine
        .options
        .keys()
        .any(|name| name.eq_ignore_ascii_case("Ponder"))
    {
        return Err(
            "the Ponder UCI option is controlled by --ponder; remove the generic Ponder option"
                .into(),
        );
    }
    if ponder {
        engine
            .options
            .insert("Ponder".into(), UciOptionValue::Check(true));
    }
    Ok(())
}

/// The name a progress block calls an engine: its label when it has one, and
/// the executable's file stem otherwise, which is what the PGN roster uses.
pub(crate) fn engine_display_name(engine: &EngineLaunchSpec) -> String {
    engine.label.clone().unwrap_or_else(|| {
        engine
            .executable
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("engine")
            .to_owned()
    })
}

/// The sample class to write for one game of a counted group.
///
/// Saying that the runner abandoned this game is what lets a PGN replay leave
/// it out of the sample, exactly as the checkpoint beside it already does.
pub(crate) fn sample_class(scorable: bool, counted: &'static str) -> &'static str {
    if scorable { counted } else { UNSCORABLE_SAMPLE }
}

/// A paired game's journal record and the `ColosseumSample` its PGN carries.
///
/// The journal keeps the class of the pair or iteration the game was played
/// in, with `scorable: false` as the mark of a game nobody could score: a
/// resume rebuilds pairs and iterations by class, and a game filed under
/// `unscorable` instead would take its pair, or its whole iteration, with it.
/// The PGN has one tag for both facts, and there `unscorable` wins, because a
/// reader of the export must leave the game out.
pub(crate) fn paired_game_entry(
    game: &match_runner::MatchGame,
    class: &'static str,
    iteration: Option<u32>,
) -> (crate::journal::GameRecord, &'static str) {
    (
        game.journal_record(class, iteration),
        sample_class(game.scorable, class),
    )
}

/// Add tags to a rendered game's header.
///
/// Some facts about a game are only known after it was played: whether its
/// pair still counted towards the official sample, or which SPSA iteration it
/// belonged to. They were written as a comment line above the game, which a
/// PGN reader attributes to whatever came before it — so the marker was there
/// but no reader could act on it. A tag inside the header can be read.
pub(crate) fn with_header_tags(pgn: &str, tags: &[(&str, &str)]) -> String {
    let rendered = tags
        .iter()
        .map(|(name, value)| format!("[{name} \"{value}\"]"))
        .collect::<Vec<_>>();
    let mut lines = pgn.lines().map(str::to_owned).collect::<Vec<_>>();
    // The header is the leading run of tag pairs; the blank line after it
    // separates them from the movetext.
    let insert = lines
        .iter()
        .position(|line| !line.starts_with('['))
        .unwrap_or(lines.len());
    for (offset, tag) in rendered.into_iter().enumerate() {
        lines.insert(insert + offset, tag);
    }
    lines.join("\n")
}

/// Two-sided 95% confidence multiplier, the one every progress block reports.
pub(crate) const Z95: f64 = 1.959_963_984_540_054;

/// How often a run asks whether a progress block is due.
///
/// The answer is decided by units and by the time floor; this only bounds how
/// late a due block can be, so it is an implementation detail rather than a
/// reporting interval.
pub(crate) const PROGRESS_POLL: Duration = Duration::from_millis(250);

/// Publish one progress block.
///
/// The console, the append-only trajectory and `status` must never disagree
/// about where a run stands, so all three are written from the same value. A
/// progress report is diagnostics: failing to record one says so and does not
/// stop the run.
pub(crate) fn publish_progress(
    block: &ProgressBlock,
    writer: &RunWriter,
    recorder: &mut RunRecorder,
) {
    show_progress(block, writer);
    if let Err(error) = recorder.update_progress(block.clone()) {
        eprintln!("run record failed: {error}");
    }
}

/// The console and the trajectory, for a command whose run recorder is owned
/// by its observer. The `run.log` line goes through the run's writer, so
/// printing a block never waits on the disk.
pub(crate) fn show_progress(block: &ProgressBlock, writer: &RunWriter) {
    eprint!("{}", block.render());
    eprintln!("{}", progress::BLOCK_SEPARATOR);
    log_event(writer, &block.log_event());
}

/// Append one human event to `run.log` through the run's writer. A failed
/// write surfaces at the next commit, which stops the run.
pub(crate) fn log_event(writer: &RunWriter, event: &Value) {
    let mut line = serde_json::to_vec(event).expect("a run.log event is serializable");
    line.push(b'\n');
    let _ = writer.log(line);
}

/// The paired sample a run has committed so far.
///
/// `sprt` and `calibrate` both report the same paired evidence, and both must
/// report the numbers their own final result will report, so this is computed
/// from the committed games with the same pairing rule and handed to the same
/// estimator.
#[derive(Debug, Clone, Default)]
pub(crate) struct PairedProgress {
    pub(crate) pairs: u32,
    pub(crate) scored_games: u32,
    pub(crate) wins: u32,
    pub(crate) draws: u32,
    pub(crate) losses: u32,
    pub(crate) vector: PentanomialVector,
    pub(crate) faults: MatchFaultCounts,
    /// Games waiting for the other colour of their pair: at most one per game
    /// in flight, so the state stays the size of the concurrency, not the run.
    halves: BTreeMap<u32, HalfPair>,
}

#[derive(Debug, Clone, Copy)]
struct HalfPair {
    number: u32,
    scorable: bool,
    result: PairGameResult,
}

impl PairedProgress {
    /// Pair committed games the way the schedule played them: the odd game of
    /// a pair had engine A as White, the even one the same opening reversed.
    pub(crate) fn from_games(games: &[match_runner::MatchGame]) -> Self {
        let mut progress = Self::default();
        for game in games {
            progress.add_game(game);
        }
        progress
    }

    /// The same, for a command that commits whole pairs.
    pub(crate) fn from_pairs(pairs: &[CompletePair<match_runner::MatchGame>]) -> Self {
        let mut progress = Self::default();
        for pair in pairs {
            progress.add_pair(pair);
        }
        progress
    }

    pub(crate) fn add_pair(&mut self, pair: &CompletePair<match_runner::MatchGame>) {
        self.add_game(&pair.first);
        self.add_game(&pair.second);
    }

    /// Count one committed game, and close its pair if the other colour is
    /// already in. The order games arrive in does not matter.
    pub(crate) fn add_game(&mut self, game: &match_runner::MatchGame) {
        record_fault(&mut self.faults, game.white, game.fault.as_ref());
        let result = result_for_engine_a(game.white, game.result);
        if game.scorable {
            self.scored_games += 1;
            match result {
                PairGameResult::Win => self.wins += 1,
                PairGameResult::Draw => self.draws += 1,
                PairGameResult::Loss => self.losses += 1,
            }
        }
        let half = HalfPair {
            number: game.number,
            scorable: game.scorable,
            result,
        };
        let pair = game.number.div_ceil(2);
        let Some(other) = self.halves.remove(&pair) else {
            self.halves.insert(pair, half);
            return;
        };
        let (first, second) = if other.number < half.number {
            (other, half)
        } else {
            (half, other)
        };
        if first.scorable && second.scorable && first.number + 1 == second.number {
            self.vector.record_pair(first.result, second.result);
            self.pairs += 1;
        }
    }

    /// The lines both commands share, in the order an operator reads them:
    /// the size of the sample, what it is worth, then how it was reached.
    pub(crate) fn add_fields(&self, block: &mut ProgressBlock, policy: FaultPolicy) {
        // A run that counts games has the game count in its headline already;
        // what it does not say is how many of them are complete pairs, which
        // is what every figure below is computed over.
        match block.unit {
            ProgressUnit::Games => {
                block.field("pairs", format!("{} complete", self.pairs));
            }
            _ => {
                block.field("games", self.scored_games.to_string());
            }
        }
        match pentanomial_statistics(&self.vector, Z95) {
            Ok(statistics) => {
                block
                    .field(
                        "Elo",
                        progress::estimate(
                            statistics.logistic_elo.elo,
                            statistics.logistic_elo.lower,
                            statistics.logistic_elo.upper,
                        ),
                    )
                    .field(
                        "nElo",
                        progress::estimate(
                            statistics.normalized_elo.elo,
                            statistics.normalized_elo.lower,
                            statistics.normalized_elo.upper,
                        ),
                    );
            }
            Err(error) => {
                // Both models come from the one estimator, so both are absent
                // together. Saying so twice keeps the block's shape stable
                // instead of dropping a line a reader is looking for.
                block
                    .field("Elo", format!("unavailable: {error}"))
                    .field("nElo", format!("unavailable: {error}"));
            }
        }
        block
            .field(
                "W/D/L",
                format!("{}/{}/{}", self.wins, self.draws, self.losses),
            )
            .field("Ptnml", format!("{:?}", self.vector.counts()))
            .field(
                "faults",
                format!(
                    "{}; {}",
                    fault_counts_text(self.faults),
                    fault_allowance_text(policy, self.faults, u64::from(self.scored_games))
                ),
            );
    }
}

/// Resolve the slot allocation from the two mutually exclusive flags.
///
/// Sharing is the default: without pondering only one engine of a game
/// searches at any moment, so a disjoint allocation would leave half the pool
/// waiting on a pipe. Clap enforces both that the flags never appear together
/// and that `--ponder` names `--cores-per-engine`, so this cannot see a
/// contradictory pair.
pub(crate) fn resolve_slot_allocation(
    cores_per_game: Option<u32>,
    cores_per_engine: Option<u32>,
) -> SlotAllocation {
    match cores_per_engine {
        Some(cores_per_engine) => SlotAllocation::PerEngine {
            cores_per_engine: cores_per_engine as usize,
        },
        None => SlotAllocation::Shared {
            cores_per_game: cores_per_game.unwrap_or(1) as usize,
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_match_engine(
    executable: PathBuf,
    label: Option<String>,
    arguments: Vec<OsString>,
    cwd: Option<PathBuf>,
    environment: Vec<String>,
    options: Vec<String>,
    buttons: Vec<String>,
    cores: Option<String>,
) -> Result<EngineLaunchSpec, crate::EngineArgsError> {
    EngineArgs {
        executable,
        label,
        arguments,
        cwd,
        environment,
        options,
        buttons,
        cores,
    }
    .resolve()
}

pub(crate) fn print_output(output: &MachineOutput<'_>, machine: bool) {
    if machine {
        print_json(output);
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(output).expect("machine output is serializable")
        );
    }
}

pub(crate) fn print_json(output: &MachineOutput<'_>) {
    println!(
        "{}",
        serde_json::to_string(output).expect("machine output is serializable")
    );
}

#[cfg(test)]
mod fault_text_tests {
    use super::*;

    #[test]
    fn each_sides_fault_count_is_separated_by_a_hyphen() {
        let faults = MatchFaultCounts {
            engine_a: 1,
            engine_b: 3,
            time_losses_a: 0,
            time_losses_b: 2,
            infrastructure: 0,
        };
        assert_eq!(fault_counts_text(faults), "time: 0-2; other: 1-1");
        let players = Players {
            a: "b22core".into(),
            b: "b22base".into(),
        };
        assert_eq!(players.to_string(), "b22core vs. b22base");
    }
}
