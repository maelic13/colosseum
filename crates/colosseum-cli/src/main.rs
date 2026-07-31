//! Independent headless composition root for Colosseum CLI.

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use colosseum_application::{
    CheckEngine, ComplianceStatus, InspectEngine, RuntimeParticipant, UciOptionSchema,
};
use colosseum_cli::EngineArgs;
use colosseum_core::ParticipantId;
use colosseum_uci::UciSessionFactory;

#[derive(Debug, Parser)]
#[command(
    name = "colosseum-cli",
    version,
    about = "Run reproducible UCI chess-engine tests and experiments",
    long_about = "A headless harness for inspecting, testing and comparing ordinary UCI chess-engine executables."
)]
struct Cli {
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
    match Cli::parse().command {
        Command::Engine(command) => run_engine(command.command).await,
    }
}

async fn run_engine(command: EngineAction) -> ExitCode {
    let invocation = match &command {
        EngineAction::Inspect(invocation) | EngineAction::Check(invocation) => invocation,
    };
    let launch = match invocation.engine.resolve() {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let participant = RuntimeParticipant {
        id: ParticipantId::from_u128(1),
        launch,
    };
    match command {
        EngineAction::Inspect(_) => {
            match InspectEngine::execute(&UciSessionFactory, &participant).await {
                Ok(inspection) => {
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
                    for check in &report.checks {
                        let status = match check.status {
                            ComplianceStatus::Pass => "PASS",
                            ComplianceStatus::Fail => "FAIL",
                            ComplianceStatus::Skipped => "SKIP",
                        };
                        println!("[{status}] {} — {}", check.requirement, check.detail);
                    }
                    if report.success {
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
