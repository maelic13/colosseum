//! Repository-owned ordinary UCI executable for hermetic path-only acceptance.

use std::io::{BufRead, Write};
use std::time::Duration;

#[derive(Debug, Default)]
struct FixtureArgs {
    sleep_ms: u64,
    /// Spin this long on each `go`, consuming CPU the way a search does.
    busy_ms: u64,
    crash_on_go: bool,
    hang_on_go: bool,
    legal_sequence: bool,
    append_pid_file: bool,
    pid_file: Option<std::path::PathBuf>,
    /// Wait this long before answering `uci`: time spent starting up.
    uciok_after_ms: u64,
    /// Append every command received, with this process's identifier, here.
    log_commands: Option<std::path::PathBuf>,
    /// Wait this long after `quit` before exiting: time spent shutting down.
    exit_after_quit_ms: u64,
    /// A search in timed phases: wait, first `info`, wait, last `info`
    /// reporting `report_time_ms`, wait, `bestmove`. Each line is flushed as
    /// it is written, so each delay lands in exactly one round-trip phase.
    phases: Option<Phases>,
    /// Scripted games, one per line as `START | MOVE MOVE …`, where START is
    /// a FEN or `startpos`. From a script's start position with N moves
    /// played, the fixture answers with the script's move N + 1.
    script: Option<std::path::PathBuf>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Phases {
    first_info_ms: u64,
    between_info_ms: u64,
    bestmove_after_info_ms: u64,
    report_time_ms: u64,
    /// Write a line longer than the protocol allows just before `bestmove`.
    overlong_before_bestmove: bool,
}

fn arguments() -> FixtureArgs {
    let mut parsed = FixtureArgs::default();
    for argument in std::env::args().skip(1) {
        if let Some(value) = argument.strip_prefix("--sleep-ms=") {
            parsed.sleep_ms = value.parse().expect("--sleep-ms needs an integer");
        } else if let Some(value) = argument.strip_prefix("--busy-ms=") {
            parsed.busy_ms = value.parse().expect("--busy-ms needs an integer");
        } else if argument == "--crash-on-go" {
            parsed.crash_on_go = true;
        } else if argument == "--hang-on-go" {
            parsed.hang_on_go = true;
        } else if argument == "--legal-sequence" {
            parsed.legal_sequence = true;
        } else if argument == "--overlong-before-bestmove" {
            parsed
                .phases
                .get_or_insert_with(Phases::default)
                .overlong_before_bestmove = true;
        } else if argument == "--append-pid-file" {
            parsed.append_pid_file = true;
        } else if let Some(value) = argument.strip_prefix("--pid-file=") {
            parsed.pid_file = Some(value.into());
        } else if let Some(value) = argument.strip_prefix("--script=") {
            parsed.script = Some(value.into());
        } else if let Some(value) = argument.strip_prefix("--log-commands=") {
            parsed.log_commands = Some(value.into());
        } else if let Some(value) = argument.strip_prefix("--uciok-after-ms=") {
            parsed.uciok_after_ms = value.parse().expect("--uciok-after-ms needs an integer");
        } else if let Some(value) = argument.strip_prefix("--exit-after-quit-ms=") {
            parsed.exit_after_quit_ms = value
                .parse()
                .expect("--exit-after-quit-ms needs an integer");
        } else if let Some((name, value)) = argument
            .strip_prefix("--")
            .and_then(|rest| rest.split_once('='))
            .filter(|(name, _)| {
                matches!(
                    *name,
                    "first-info-ms"
                        | "between-info-ms"
                        | "bestmove-after-info-ms"
                        | "report-time-ms"
                )
            })
        {
            let value = value.parse().expect("a phase delay needs an integer");
            let phases = parsed.phases.get_or_insert_with(Phases::default);
            match name {
                "first-info-ms" => phases.first_info_ms = value,
                "between-info-ms" => phases.between_info_ms = value,
                "bestmove-after-info-ms" => phases.bestmove_after_info_ms = value,
                _ => phases.report_time_ms = value,
            }
        } else {
            panic!("unknown fixture argument: {argument}");
        }
    }
    parsed
}

fn main() -> std::io::Result<()> {
    let arguments = arguments();
    if let Some(path) = &arguments.pid_file {
        if arguments.append_pid_file {
            use std::fs::OpenOptions;
            let mut file = OpenOptions::new().create(true).append(true).open(path)?;
            writeln!(file, "{}", std::process::id())?;
        } else {
            std::fs::write(path, std::process::id().to_string())?;
        }
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let mut searching = false;
    let mut position = String::from("position startpos");
    let mut command_log = match &arguments.log_commands {
        Some(path) => Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?,
        ),
        None => None,
    };
    let script = match &arguments.script {
        Some(path) => read_script(path)?,
        None => Vec::new(),
    };
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if let Some(log) = command_log.as_mut() {
            writeln!(log, "{} {line}", std::process::id())?;
        }
        match line {
            "uci" => {
                std::thread::sleep(Duration::from_millis(arguments.uciok_after_ms));
                writeln!(stdout, "id name Colosseum path-only fixture")?;
                writeln!(stdout, "id author Colosseum")?;
                writeln!(
                    stdout,
                    "option name Hash type spin default 16 min 1 max 1024"
                )?;
                writeln!(stdout, "uciok")?;
            }
            "isready" => writeln!(stdout, "readyok")?,
            "quit" => {
                std::thread::sleep(Duration::from_millis(arguments.exit_after_quit_ms));
                break;
            }
            "stop" if searching => {
                writeln!(stdout, "bestmove e2e4")?;
                searching = false;
            }
            _ if line.starts_with("position ") => position = line.to_owned(),
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
                let spin = std::time::Instant::now();
                while spin.elapsed() < Duration::from_millis(arguments.busy_ms) {
                    std::hint::spin_loop();
                }
                if let Some(phases) = arguments.phases {
                    write_timed_search(&mut stdout, phases)?;
                }
                if arguments.script.is_some() {
                    writeln!(stdout, "info depth 1 nodes 1 score cp 0")?;
                    writeln!(stdout, "bestmove {}", scripted_move(&script, &position))?;
                } else if arguments.legal_sequence {
                    write_legal_bestmove(&mut stdout, &position, arguments.phases.is_none())?;
                } else {
                    writeln!(stdout, "bestmove e2e4")?;
                }
            }
            _ => {}
        }
        stdout.flush()?;
    }
    Ok(())
}

fn write_timed_search(output: &mut impl Write, phases: Phases) -> std::io::Result<()> {
    std::thread::sleep(Duration::from_millis(phases.first_info_ms));
    writeln!(output, "info depth 1 time 0 nodes 1 score cp 0")?;
    output.flush()?;
    std::thread::sleep(Duration::from_millis(phases.between_info_ms));
    writeln!(
        output,
        "info depth 2 time {} nodes 2 score cp 0",
        phases.report_time_ms
    )?;
    output.flush()?;
    std::thread::sleep(Duration::from_millis(phases.bestmove_after_info_ms));
    if phases.overlong_before_bestmove {
        writeln!(output, "info string {}", "x".repeat(70 * 1024))?;
    }
    Ok(())
}

fn write_legal_bestmove(
    output: &mut impl Write,
    position: &str,
    with_info: bool,
) -> std::io::Result<()> {
    let moves = position
        .split_once(" moves ")
        .map_or(0, |(_, moves)| moves.split_whitespace().count());
    let best = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]
        .get(moves)
        .copied()
        .unwrap_or("0000");
    if with_info {
        writeln!(output, "info depth 1 nodes 1 score cp 0")?;
    }
    writeln!(output, "bestmove {best}")
}

/// One scripted game: its start (`startpos` or the first four FEN fields)
/// and its moves.
type ScriptedGame = (String, Vec<String>);

fn read_script(path: &std::path::Path) -> std::io::Result<Vec<ScriptedGame>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .filter_map(|line| line.split_once('|'))
        .map(|(start, moves)| {
            (
                script_start(start),
                moves.split_whitespace().map(str::to_owned).collect(),
            )
        })
        .collect())
}

/// A start position as scripts and `position` commands are compared: the
/// placement, side, castling and en passant fields, without the counters.
fn script_start(start: &str) -> String {
    start
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

fn scripted_move(script: &[ScriptedGame], position: &str) -> String {
    let rest = position.strip_prefix("position ").unwrap_or(position);
    let (start, moves) = rest.split_once(" moves ").unwrap_or((rest, ""));
    let start = script_start(start.strip_prefix("fen ").unwrap_or(start));
    let played = moves.split_whitespace().count();
    script
        .iter()
        .find(|(scripted, _)| *scripted == start)
        .and_then(|(_, moves)| moves.get(played).cloned())
        .unwrap_or_else(|| "0000".into())
}
