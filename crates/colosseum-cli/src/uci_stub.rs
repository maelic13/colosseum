//! Hidden deterministic UCI engine used by the shipped executable self-test.

use std::path::PathBuf;
use std::process::Stdio;

use clap::{Args, ValueEnum};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum StubMode {
    #[default]
    Conforming,
    Flood,
    LongLine,
    IgnoreQuit,
    Descendant,
    OrphanChild,
}

#[derive(Debug, Args)]
pub struct StubArgs {
    #[arg(long, value_enum, default_value_t = StubMode::Conforming)]
    mode: StubMode,
    #[arg(long)]
    pid_file: Option<PathBuf>,
}

pub async fn run(args: StubArgs) -> std::io::Result<()> {
    if matches!(args.mode, StubMode::OrphanChild) {
        std::future::pending::<()>().await;
        return Ok(());
    }

    let mut descendant = if matches!(args.mode, StubMode::Descendant) {
        let child = std::process::Command::new(std::env::current_exe()?)
            .args(["__uci-stub", "--mode", "orphan-child"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        if let Some(path) = &args.pid_file {
            std::fs::write(path, child.id().to_string())?;
        }
        Some(child)
    } else {
        None
    };

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();
    let mut position = String::from("position startpos");
    let mut searching = false;
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        match line {
            "uci" => {
                match args.mode {
                    StubMode::Flood => {
                        for index in 0..4_000 {
                            stdout
                                .write_all(
                                    format!("info string stdout-flood-{index:04}\n").as_bytes(),
                                )
                                .await?;
                            stderr
                                .write_all(format!("stderr-flood-{index:04}\n").as_bytes())
                                .await?;
                        }
                    }
                    StubMode::LongLine => {
                        stdout
                            .write_all(&vec![b'x'; colosseum_uci::MAX_PROTOCOL_LINE_BYTES + 1])
                            .await?;
                        stdout.write_all(b"\n").await?;
                    }
                    _ => {}
                }
                stdout
                    .write_all(b"id name Colosseum deterministic stub\n")
                    .await?;
                stdout.write_all(b"id author Colosseum\n").await?;
                stdout
                    .write_all(b"option name Hash type spin default 16 min 1 max 1024\nuciok\n")
                    .await?;
                stdout.flush().await?;
                stderr.flush().await?;
            }
            "isready" => {
                stdout.write_all(b"readyok\n").await?;
                stdout.flush().await?;
            }
            "ucinewgame" | "stop" if line == "ucinewgame" => {}
            "stop" => {
                if searching {
                    write_bestmove(&mut stdout, &position).await?;
                    searching = false;
                }
            }
            "quit" if matches!(args.mode, StubMode::IgnoreQuit | StubMode::Descendant) => {
                std::future::pending::<()>().await;
            }
            "quit" => break,
            _ if line.starts_with("position ") => position = line.to_owned(),
            _ if line.starts_with("go ") => {
                if line.contains("movetime 10000") {
                    searching = true;
                } else {
                    write_bestmove(&mut stdout, &position).await?;
                }
            }
            _ => {}
        }
    }
    if let Some(child) = descendant.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

async fn write_bestmove(stdout: &mut tokio::io::Stdout, position: &str) -> std::io::Result<()> {
    let moves = position
        .split_once(" moves ")
        .map_or(0, |(_, moves)| moves.split_whitespace().count());
    let best = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]
        .get(moves)
        .copied()
        .unwrap_or("0000");
    stdout
        .write_all(format!("info depth 1 nodes 1 score cp 0\nbestmove {best}\n").as_bytes())
        .await?;
    stdout.flush().await
}
