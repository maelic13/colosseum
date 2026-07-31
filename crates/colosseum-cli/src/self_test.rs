//! End-to-end checks that travel inside the shipped CLI executable.

use std::path::{Path, PathBuf};
use std::time::Duration;

use colosseum_application::{CheckEngine, RuntimeParticipant};
use colosseum_cli::{built_in_defaults, resolve_config};
use colosseum_core::ParticipantId;
use colosseum_uci::{
    EngineProcess, GoLimits, SpawnOptions, UciPosition, UciSessionFactory, process_is_alive,
};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Serialize)]
pub struct SelfTestReport {
    pub executable: PathBuf,
    pub checks: Vec<SelfTestCheck>,
    pub success: bool,
}

#[derive(Debug, Serialize)]
pub struct SelfTestCheck {
    pub name: &'static str,
    pub success: bool,
    pub detail: String,
}

pub async fn execute() -> SelfTestReport {
    let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("colosseum-cli"));
    let mut checks = Vec::new();
    check(
        &mut checks,
        "protocol-compliance",
        compliance(&executable).await,
    );
    check(
        &mut checks,
        "bounded-pipes",
        bounded_pipes(&executable).await,
    );
    check(
        &mut checks,
        "process-tree-reaping",
        containment(&executable).await,
    );
    check(&mut checks, "persistence-failure", persistence_failure());
    check(&mut checks, "short-match", short_match(&executable).await);
    let success = checks.iter().all(|item| item.success);
    SelfTestReport {
        executable,
        checks,
        success,
    }
}

fn check(checks: &mut Vec<SelfTestCheck>, name: &'static str, result: Result<String, String>) {
    match result {
        Ok(detail) => checks.push(SelfTestCheck {
            name,
            success: true,
            detail,
        }),
        Err(detail) => checks.push(SelfTestCheck {
            name,
            success: false,
            detail,
        }),
    }
}

fn spawn(executable: &Path, mode: &str) -> SpawnOptions {
    SpawnOptions {
        path: executable.to_path_buf(),
        args: vec!["__uci-stub".into(), "--mode".into(), mode.into()],
        ..SpawnOptions::default()
    }
}

async fn compliance(executable: &Path) -> Result<String, String> {
    let participant = RuntimeParticipant {
        id: ParticipantId::from_u128(1),
        launch: colosseum_application::EngineLaunchSpec {
            executable: executable.to_path_buf(),
            arguments: vec!["__uci-stub".into(), "--mode".into(), "conforming".into()],
            ..colosseum_application::EngineLaunchSpec::path_only(executable.to_path_buf())
        },
    };
    let report = CheckEngine::execute(&UciSessionFactory, &participant)
        .await
        .map_err(|error| error.to_string())?;
    if report.success {
        Ok("handshake, ready, search, stop, new-game and shutdown passed".into())
    } else {
        Err(format!("compliance report failed: {:?}", report.checks))
    }
}

async fn bounded_pipes(executable: &Path) -> Result<String, String> {
    let mut flood = EngineProcess::spawn(spawn(executable, "flood"))
        .await
        .map_err(|error| error.to_string())?;
    flood
        .handshake(Duration::from_secs(5))
        .await
        .map_err(|error| error.to_string())?;
    let stderr = flood.stderr_tail();
    if stderr.len() > 40 || stderr.iter().any(|line| line.len() > 16_400) {
        return Err("stderr tail exceeded its documented bound".into());
    }
    flood
        .quit(Duration::from_secs(1))
        .await
        .map_err(|error| error.to_string())?;

    let mut long_line = EngineProcess::spawn(spawn(executable, "long-line"))
        .await
        .map_err(|error| error.to_string())?;
    let error = long_line
        .handshake(Duration::from_secs(2))
        .await
        .expect_err("over-limit line must fail");
    long_line.kill().await.map_err(|error| error.to_string())?;
    if !error.to_string().contains("exceeds") {
        return Err(format!("wrong long-line classification: {error}"));
    }
    Ok("both pipes drained; tails and protocol lines remained bounded".into())
}

async fn containment(executable: &Path) -> Result<String, String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let pid_file = root.path().join("descendant.pid");
    let mut options = spawn(executable, "descendant");
    options
        .args
        .extend(["--pid-file".into(), pid_file.display().to_string()]);
    let mut process = EngineProcess::spawn(options)
        .await
        .map_err(|error| error.to_string())?;
    let root_pid = process.id().ok_or("engine PID unavailable")?;
    process
        .handshake(Duration::from_secs(2))
        .await
        .map_err(|error| error.to_string())?;
    let descendant_pid: u32 = std::fs::read_to_string(&pid_file)
        .map_err(|error| error.to_string())?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    if process.quit(Duration::from_millis(150)).await.is_ok() {
        return Err("ignore-quit stub unexpectedly exited cleanly".into());
    }
    for _ in 0..50 {
        if !process_is_alive(root_pid) && !process_is_alive(descendant_pid) {
            return Ok("ignored quit escalated and reaped the engine plus descendant".into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err(format!(
        "process tree survived: root={root_pid}, descendant={descendant_pid}"
    ))
}

fn persistence_failure() -> Result<String, String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let blocked = root.path().join("not-a-directory");
    std::fs::write(&blocked, b"block").map_err(|error| error.to_string())?;
    let config = resolve_config(
        built_in_defaults(),
        None,
        json!({"self_test": true}),
        &[],
        root.path(),
        &[],
    )
    .map_err(|error| error.to_string())?;
    if config.write_to(&blocked).is_ok() {
        Err("required persistence failure was swallowed".into())
    } else {
        Ok("required configuration persistence failure propagated".into())
    }
}

async fn short_match(executable: &Path) -> Result<String, String> {
    let mut white = EngineProcess::spawn(spawn(executable, "conforming"))
        .await
        .map_err(|error| error.to_string())?;
    let mut black = EngineProcess::spawn(spawn(executable, "conforming"))
        .await
        .map_err(|error| error.to_string())?;
    for engine in [&mut white, &mut black] {
        engine
            .handshake(Duration::from_secs(2))
            .await
            .map_err(|error| error.to_string())?;
        engine
            .is_ready(Duration::from_secs(2))
            .await
            .map_err(|error| error.to_string())?;
    }
    let expected = ["e2e4", "e7e5", "g1f3", "b8c6"];
    let mut moves = Vec::new();
    for (ply, expected_move) in expected.iter().enumerate() {
        let engine = if ply % 2 == 0 { &mut white } else { &mut black };
        let output = engine
            .search(
                &UciPosition::StartPos {
                    moves: moves.clone(),
                },
                &GoLimits::Depth(1),
                Duration::from_secs(2),
                |_| {},
            )
            .await
            .map_err(|error| error.to_string())?;
        if output.best_move != *expected_move {
            return Err(format!(
                "ply {ply} returned {}, expected {expected_move}",
                output.best_move
            ));
        }
        moves.push(output.best_move);
    }
    white
        .quit(Duration::from_secs(1))
        .await
        .map_err(|error| error.to_string())?;
    black
        .quit(Duration::from_secs(1))
        .await
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "completed deterministic {}-ply engine exchange",
        moves.len()
    ))
}
