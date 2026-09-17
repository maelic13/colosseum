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

    /// Invalidate after more engine-attributable faults than this value.
    #[arg(long, default_value_t = 0)]
    pub(crate) max_engine_faults: u32,
    /// Invalidate after more time losses than this value.
    #[arg(long, default_value_t = 0)]
    pub(crate) max_time_losses: u32,

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
    /// Seconds between live progress reports on standard error.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) progress_interval_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum BookOrderArg {
    Sequential,
    Random,
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

/// The sample class to write for one game of a counted group.
///
/// Saying that the runner abandoned this game is what lets a PGN replay leave
/// it out of the sample, exactly as the checkpoint beside it already does.
pub(crate) fn sample_class(scorable: bool, counted: &'static str) -> &'static str {
    if scorable { counted } else { UNSCORABLE_SAMPLE }
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
