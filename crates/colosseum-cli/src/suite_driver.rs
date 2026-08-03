use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::{Args, ValueEnum};
use colosseum_application::{
    ApplicationError, PortFuture, RunSuite, RuntimeParticipant, SearchLimit,
    SuiteBaselineComparison, SuiteDesign, SuitePositionResult, SuiteProgress, SuiteReport,
    compare_suite_baseline,
};
use colosseum_core::ParticipantId;
use colosseum_engine::{SuiteInputFormat, parse_suite_input};
use colosseum_uci::AffinityUciSessionFactory;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    EngineArgs, OfficialSample, RunDirectory, RunRecorder, RunStatus, built_in_defaults,
    resolve_config,
};

#[derive(Debug, Args)]
pub struct SuiteCommand {
    #[command(flatten)]
    engine: EngineArgs,
    /// EPD or line-oriented FEN position set.
    input: PathBuf,
    /// Override input format detection from the extension.
    #[arg(long, value_enum)]
    format: Option<SuiteFormatArg>,
    /// Fixed nodes per position.
    #[arg(long, conflicts_with_all = ["movetime_ms", "depth"])]
    nodes: Option<u64>,
    /// Fixed search time per position.
    #[arg(long, conflicts_with_all = ["nodes", "depth"])]
    movetime_ms: Option<u64>,
    /// Fixed search depth per position.
    #[arg(long, conflicts_with_all = ["nodes", "movetime_ms"])]
    depth: Option<u32>,
    /// Safety deadline per position; derived from the limit when omitted.
    #[arg(long)]
    deadline_ms: Option<u64>,
    /// Previous compatible suite result.json to compare.
    #[arg(long)]
    baseline: Option<PathBuf>,
    /// Self-contained run directory; an existing matching directory resumes.
    #[arg(long = "dir")]
    run_directory: Option<PathBuf>,
    /// Archive an existing --dir and start a fresh run there.
    #[arg(long, requires = "run_directory")]
    restart: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SuiteFormatArg {
    Epd,
    Fen,
}

impl From<SuiteFormatArg> for SuiteInputFormat {
    fn from(value: SuiteFormatArg) -> Self {
        match value {
            SuiteFormatArg::Epd => Self::Epd,
            SuiteFormatArg::Fen => Self::Fen,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SuiteCheckpoint {
    position_set_sha256: String,
    search_sha256: String,
    results: Vec<SuitePositionResult>,
}

#[derive(Debug, Serialize)]
struct SuiteOutput<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    run_directory: &'a Path,
    report: &'a SuiteReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    comparison: Option<&'a SuiteBaselineComparison>,
}

struct DurableSuiteProgress {
    directory: Arc<RunDirectory>,
    checkpoint: Mutex<SuiteCheckpoint>,
    recorder: Arc<Mutex<Option<RunRecorder>>>,
}

impl SuiteProgress for DurableSuiteProgress {
    fn commit(&self, result: &SuitePositionResult) -> PortFuture<'_, Result<(), ApplicationError>> {
        let result = result.clone();
        Box::pin(async move {
            let committed = {
                let mut checkpoint = self.checkpoint.lock().map_err(lock_error)?;
                if checkpoint
                    .results
                    .iter()
                    .any(|item| item.index == result.index)
                {
                    return Err(ApplicationError::InfrastructureFault {
                        operation: "suite-checkpoint".into(),
                        message: format!("position {} was committed twice", result.index),
                    });
                }
                checkpoint.results.push(result);
                checkpoint.results.sort_by_key(|item| item.index);
                self.directory
                    .write_checkpoint(&*checkpoint)
                    .map_err(|error| application_infrastructure("write suite checkpoint", error))?;
                checkpoint.results.len() as u64
            };
            let mut recorder = self.recorder.lock().map_err(lock_error)?;
            recorder
                .as_mut()
                .ok_or_else(|| ApplicationError::InfrastructureFault {
                    operation: "suite-run-record".into(),
                    message: "run recorder is unavailable".into(),
                })?
                .update_sample(OfficialSample {
                    committed_units: committed,
                    ..OfficialSample::default()
                })
                .map_err(|error| application_infrastructure("update suite run record", error))
        })
    }
}

pub async fn run(command: SuiteCommand, machine: bool, dry_run: bool) -> ExitCode {
    match prepare_and_run(command, machine, dry_run).await {
        Ok(code) => code,
        Err((code, message)) => {
            eprintln!("{message}");
            code
        }
    }
}

async fn prepare_and_run(
    command: SuiteCommand,
    machine: bool,
    dry_run: bool,
) -> Result<ExitCode, (ExitCode, String)> {
    let limit = search_limit(&command).map_err(configuration)?;
    let deadline_ms = command
        .deadline_ms
        .unwrap_or_else(|| default_deadline(&limit));
    if deadline_ms == 0 {
        return Err(configuration("--deadline-ms must be positive".into()));
    }
    if let SearchLimit::MoveTimeMs(movetime) = &limit
        && deadline_ms <= *movetime
    {
        return Err(configuration(
            "--deadline-ms must exceed --movetime-ms so protocol overhead has a safety margin"
                .into(),
        ));
    }
    let format = command
        .format
        .unwrap_or_else(|| detect_format(&command.input));
    let input_bytes = std::fs::read(&command.input).map_err(|error| {
        configuration(format!(
            "cannot read suite input {}: {error}",
            command.input.display()
        ))
    })?;
    let input_text = std::str::from_utf8(&input_bytes)
        .map_err(|error| configuration(format!("suite input is not UTF-8: {error}")))?;
    let entries = parse_suite_input(input_text, format.into());
    if entries.is_empty() {
        return Err(configuration(
            "suite input contains no non-comment entries".into(),
        ));
    }
    let position_set_sha256 = hash_json(&json!({
        "parser": "colosseum-epd-fen-suite-v1",
        "format": format,
        "input_sha256": hex(&Sha256::digest(&input_bytes)),
        "entries": entries,
    }));
    let search_sha256 = hash_json(&json!({
        "search_contract": "uci-fixed-work-v1",
        "limit": limit,
        "deadline_ms": deadline_ms,
    }));
    let design = SuiteDesign {
        position_set_sha256,
        search_sha256,
        limit,
        deadline_ms,
        entries,
    };
    let launch = command
        .engine
        .resolve()
        .map_err(|error| configuration(error.to_string()))?;
    let participant = RuntimeParticipant {
        id: ParticipantId::from_u128(1),
        launch,
    };
    let current_directory = std::env::current_dir()
        .map_err(|error| configuration(format!("cannot read current directory: {error}")))?;
    let mut paths = vec![
        "/input".to_owned(),
        "/participant/launch/executable".to_owned(),
    ];
    if participant.launch.working_directory.is_some() {
        paths.push("/participant/launch/working_directory".into());
    }
    let resolved = resolve_config(
        built_in_defaults(),
        None,
        json!({
            "command": "suite",
            "input": command.input,
            "format": format,
            "participant": participant,
            "design": design,
        }),
        &[],
        &current_directory,
        &paths,
    )
    .map_err(|error| configuration(error.to_string()))?;
    let participant: RuntimeParticipant =
        serde_json::from_value(resolved.value()["participant"].clone())
            .expect("resolved suite participant retains its schema");
    let design: SuiteDesign = serde_json::from_value(resolved.value()["design"].clone())
        .expect("resolved suite design retains its schema");
    if dry_run {
        let output = json!({
            "type": "dry-run",
            "command": "suite",
            "config_sha256": resolved.sha256(),
            "resolved_configuration": resolved.value(),
            "invocations": [&participant.launch],
        });
        if machine {
            println!("{}", serde_json::to_string(&output).expect("JSON output"));
        } else {
            println!("suite dry-run configuration SHA-256: {}", resolved.sha256());
            println!("{} entries", design.entries.len());
        }
        return Ok(ExitCode::SUCCESS);
    }

    let opened = match &command.run_directory {
        Some(path) => RunDirectory::open_explicit(path, &resolved, command.restart),
        None => RunDirectory::create_unique(&current_directory, "suite", &resolved),
    }
    .map_err(|error| configuration(error.to_string()))?;
    if let Some(archived) = &opened.archived {
        eprintln!("archived previous run at {}", archived.display());
    }
    let resumed = opened.resumed;
    let directory = Arc::new(opened.directory);
    let mut recorder = if resumed {
        RunRecorder::resume(&directory)
    } else {
        RunRecorder::begin(&directory, "suite")
    }
    .map_err(|error| infrastructure_error("suite run record", error))?;
    recorder
        .set_workflow(json!({
            "position_set_sha256": design.position_set_sha256,
            "search_sha256": design.search_sha256,
            "total_entries": design.entries.len(),
            "resumed": resumed,
        }))
        .map_err(|error| infrastructure_error("suite run record", error))?;
    let recorder = Arc::new(Mutex::new(Some(recorder)));
    let checkpoint = if resumed {
        directory
            .read_checkpoint::<SuiteCheckpoint>()
            .map_err(|error| infrastructure_error("read suite checkpoint", error))?
    } else {
        SuiteCheckpoint {
            position_set_sha256: design.position_set_sha256.clone(),
            search_sha256: design.search_sha256.clone(),
            results: Vec::new(),
        }
    };
    if checkpoint.position_set_sha256 != design.position_set_sha256
        || checkpoint.search_sha256 != design.search_sha256
    {
        return Err(configuration(
            "suite checkpoint identity does not match the resolved design".into(),
        ));
    }
    let prior = checkpoint.results.clone();
    let progress = DurableSuiteProgress {
        directory: Arc::clone(&directory),
        checkpoint: Mutex::new(checkpoint),
        recorder: Arc::clone(&recorder),
    };
    let report = RunSuite::execute(
        &AffinityUciSessionFactory::new(apply_affinity),
        &participant,
        design,
        prior,
        &progress,
    )
    .await
    .map_err(|error| (ExitCode::FAILURE, format!("suite failed: {error}")))?;
    let comparison = command
        .baseline
        .as_deref()
        .map(|path| {
            read_baseline(path).and_then(|baseline| {
                compare_suite_baseline(&report, &baseline).map_err(|error| error.to_string())
            })
        })
        .transpose()
        .map_err(configuration)?;
    write_result(&directory.paths().root.join("result.json"), &report)
        .map_err(|error| infrastructure_error("write suite result", error))?;
    let final_status = if report.malformed > 0 {
        RunStatus::Invalid
    } else {
        RunStatus::Completed
    };
    recorder
        .lock()
        .map_err(|_| infrastructure_error("suite run record", "lock poisoned"))?
        .take()
        .ok_or_else(|| infrastructure_error("suite run record", "recorder unavailable"))?
        .finish(final_status)
        .map_err(|error| infrastructure_error("finish suite run record", error))?;

    if machine {
        println!(
            "{}",
            serde_json::to_string(&SuiteOutput {
                kind: "suite",
                run_directory: &directory.paths().root,
                report: &report,
                comparison: comparison.as_ref(),
            })
            .expect("suite JSON output")
        );
    } else {
        println!("run directory: {}", directory.paths().root.display());
        println!(
            "{} searched; {} passed, {} failed, {} unscored, {} malformed; pass rate {}",
            report.searched,
            report.passed,
            report.failed,
            report.unscored,
            report.malformed,
            report.pass_rate.map_or_else(
                || "unavailable".into(),
                |rate| format!("{:.2}%", rate * 100.0)
            ),
        );
        if let Some(comparison) = comparison {
            println!(
                "baseline: passed delta {:+}; changed positions {:?}",
                comparison.passed_delta, comparison.changed_positions
            );
        }
    }
    Ok(if report.failed > 0 || report.malformed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn search_limit(command: &SuiteCommand) -> Result<SearchLimit, String> {
    match (command.nodes, command.movetime_ms, command.depth) {
        (Some(value), None, None) if value > 0 => Ok(SearchLimit::Nodes(value)),
        (None, Some(value), None) if value > 0 => Ok(SearchLimit::MoveTimeMs(value)),
        (None, None, Some(value)) if value > 0 => Ok(SearchLimit::Depth(value)),
        _ => Err("select exactly one positive --nodes, --movetime-ms or --depth".into()),
    }
}

fn default_deadline(limit: &SearchLimit) -> u64 {
    match limit {
        SearchLimit::MoveTimeMs(value) => value.saturating_mul(2).saturating_add(5_000),
        SearchLimit::Nodes(_) | SearchLimit::Depth(_) => 600_000,
    }
}

fn detect_format(path: &Path) -> SuiteFormatArg {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("fen"))
    {
        SuiteFormatArg::Fen
    } else {
        SuiteFormatArg::Epd
    }
}

fn hash_json(value: &serde_json::Value) -> String {
    hex(&Sha256::digest(
        serde_json::to_vec(value).expect("suite identity JSON is serializable"),
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_baseline(path: &Path) -> Result<SuiteReport, String> {
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| format!("cannot read baseline {}: {error}", path.display()))?,
    )
    .map_err(|error| format!("invalid baseline JSON: {error}"))?;
    serde_json::from_value(value.get("report").cloned().unwrap_or(value))
        .map_err(|error| format!("baseline is not a suite report: {error}"))
}

fn write_result(path: &Path, report: &SuiteReport) -> Result<(), std::io::Error> {
    use std::io::Write;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(report).expect("suite report JSON"))?;
    file.sync_all()?;
    std::fs::rename(temporary, path)
}

fn apply_affinity(
    process_id: u32,
    allocation: &colosseum_application::CpuAllocation,
) -> Result<(), String> {
    colosseum_engine::apply_process_affinity(process_id, allocation)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> ApplicationError {
    ApplicationError::InfrastructureFault {
        operation: "suite-state".into(),
        message: "suite state lock is poisoned".into(),
    }
}

fn application_infrastructure(
    operation: &'static str,
    error: impl std::fmt::Display,
) -> ApplicationError {
    ApplicationError::InfrastructureFault {
        operation: operation.into(),
        message: error.to_string(),
    }
}

fn configuration(message: String) -> (ExitCode, String) {
    (ExitCode::from(2), format!("configuration error: {message}"))
}

fn infrastructure_error(
    operation: &'static str,
    error: impl std::fmt::Display,
) -> (ExitCode, String) {
    (
        ExitCode::from(3),
        format!("infrastructure error: {operation}: {error}"),
    )
}
