//! Repository-owned ordinary UCI executable for hermetic path-only acceptance.

use std::io::{BufRead, Write};
use std::time::Duration;

#[derive(Debug, Default)]
struct FixtureArgs {
    sleep_ms: u64,
    crash_on_go: bool,
    hang_on_go: bool,
    pid_file: Option<std::path::PathBuf>,
}

fn arguments() -> FixtureArgs {
    let mut parsed = FixtureArgs::default();
    for argument in std::env::args().skip(1) {
        if let Some(value) = argument.strip_prefix("--sleep-ms=") {
            parsed.sleep_ms = value.parse().expect("--sleep-ms needs an integer");
        } else if argument == "--crash-on-go" {
            parsed.crash_on_go = true;
        } else if argument == "--hang-on-go" {
            parsed.hang_on_go = true;
        } else if let Some(value) = argument.strip_prefix("--pid-file=") {
            parsed.pid_file = Some(value.into());
        } else {
            panic!("unknown fixture argument: {argument}");
        }
    }
    parsed
}

fn main() -> std::io::Result<()> {
    let arguments = arguments();
    if let Some(path) = &arguments.pid_file {
        std::fs::write(path, std::process::id().to_string())?;
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let mut searching = false;
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        match line {
            "uci" => {
                writeln!(stdout, "id name Colosseum path-only fixture")?;
                writeln!(stdout, "id author Colosseum")?;
                writeln!(
                    stdout,
                    "option name Hash type spin default 16 min 1 max 1024"
                )?;
                writeln!(stdout, "uciok")?;
            }
            "isready" => writeln!(stdout, "readyok")?,
            "quit" => break,
            "stop" if searching => {
                writeln!(stdout, "bestmove e2e4")?;
                searching = false;
            }
            _ if line.starts_with("go ") && line.contains("movetime 10000") => {
                searching = true;
            }
            _ if line.starts_with("go ") => {
                if arguments.crash_on_go {
                    std::process::exit(19);
                }
                if arguments.hang_on_go {
                    searching = true;
                    continue;
                }
                std::thread::sleep(Duration::from_millis(arguments.sleep_ms));
                writeln!(stdout, "bestmove e2e4")?;
            }
            _ => {}
        }
        stdout.flush()?;
    }
    Ok(())
}
