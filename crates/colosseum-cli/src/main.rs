//! Independent headless composition root for Colosseum CLI.

use std::path::Path;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use colosseum_application::{
    CheckEngine, ComplianceReport, ComplianceStatus, EngineInspection, EngineLaunchSpec,
    InspectEngine, RuntimeParticipant, UciOptionSchema,
};
use colosseum_cli::{EngineArgs, built_in_defaults, resolve_config};
use colosseum_core::ParticipantId;
use colosseum_uci::UciSessionFactory;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(
    name = "colosseum-cli",
    version,
    about = "Run reproducible UCI chess-engine tests and experiments",
    long_about = "A headless harness for inspecting, testing and comparing ordinary UCI chess-engine executables."
)]
struct Cli {
    /// Emit exactly one JSON value on stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Resolve and print configuration/invocations without launching an engine.
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect or compliance-check an ordinary UCI executable.
    Engine(EngineCommand),
}

#[derive(Debug, Args)]
struct EngineCommand {
    #[command(subcommand)]
    command: EngineAction,
}

#[derive(Debug, Subcommand)]
enum EngineAction {
    /// Print UCI identity and the option schema advertised during handshake.
    Inspect(EngineInvocation),
    /// Run the bounded UCI compliance report.
    Check(EngineInvocation),
}

#[derive(Debug, Args)]
struct EngineInvocation {
    #[command(flatten)]
    engine: EngineArgs,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Engine(command) => run_engine(command.command, cli.json, cli.dry_run).await,
    }
}

async fn run_engine(command: EngineAction, machine: bool, dry_run: bool) -> ExitCode {
    let invocation = match &command {
        EngineAction::Inspect(invocation) | EngineAction::Check(invocation) => invocation,
    };
    let action = match command {
        EngineAction::Inspect(_) => "engine-inspect",
        EngineAction::Check(_) => "engine-check",
    };
    let (launch, resolved) = match resolve_invocation(action, &invocation.engine) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if dry_run {
        let output = MachineOutput::DryRun {
            command: action,
            config_sha256: resolved.sha256(),
            resolved_configuration: resolved.value(),
            invocations: vec![&launch],
        };
        print_output(&output, machine);
        return ExitCode::SUCCESS;
    }
    let participant = RuntimeParticipant {
        id: ParticipantId::from_u128(1),
        launch,
    };
    match command {
        EngineAction::Inspect(_) => {
            match InspectEngine::execute(&UciSessionFactory, &participant).await {
                Ok(inspection) => {
                    if machine {
                        print_json(&MachineOutput::EngineInspection { inspection });
                    } else {
                        print_inspection(inspection);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("engine inspect failed: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        EngineAction::Check(_) => {
            match CheckEngine::execute(&UciSessionFactory, &participant).await {
                Ok(report) => {
                    let success = report.success;
                    if machine {
                        print_json(&MachineOutput::EngineCompliance { report });
                    } else {
                        print_compliance(&report);
                    }
                    if success {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(error) => {
                    eprintln!("engine check failed: {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn resolve_invocation(
    action: &str,
    engine: &EngineArgs,
) -> Result<(EngineLaunchSpec, colosseum_cli::ResolvedConfig), Box<dyn std::error::Error>> {
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
enum MachineOutput<'a> {
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
}

fn print_output(output: &MachineOutput<'_>, machine: bool) {
    if machine {
        print_json(output);
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(output).expect("machine output is serializable")
        );
    }
}

fn print_json(output: &MachineOutput<'_>) {
    println!(
        "{}",
        serde_json::to_string(output).expect("machine output is serializable")
    );
}

fn print_inspection(inspection: EngineInspection) {
    println!(
        "name: {}",
        inspection.name.as_deref().unwrap_or("<not reported>")
    );
    println!(
        "author: {}",
        inspection.author.as_deref().unwrap_or("<not reported>")
    );
    println!("options: {}", inspection.options.len());
    for option in inspection.options {
        println!("  {}", describe_option(&option));
    }
}

fn print_compliance(report: &ComplianceReport) {
    for check in &report.checks {
        let status = match check.status {
            ComplianceStatus::Pass => "PASS",
            ComplianceStatus::Fail => "FAIL",
            ComplianceStatus::Skipped => "SKIP",
        };
        println!("[{status}] {} — {}", check.requirement, check.detail);
    }
}

fn describe_option(option: &UciOptionSchema) -> String {
    match option {
        UciOptionSchema::Check { name, default } => {
            format!("{name}: check (default {default})")
        }
        UciOptionSchema::Spin {
            name,
            default,
            min,
            max,
        } => format!("{name}: spin (default {default}, range {min}..={max})"),
        UciOptionSchema::Combo {
            name,
            default,
            values,
        } => format!(
            "{name}: combo (default {default}, values {})",
            values.join(", ")
        ),
        UciOptionSchema::Button { name } => format!("{name}: button"),
        UciOptionSchema::String { name, default } => {
            format!("{name}: string (default {default:?})")
        }
    }
}
