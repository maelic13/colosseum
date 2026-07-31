//! Repository-owned ordinary UCI executable for hermetic path-only acceptance.

use std::io::{BufRead, Write};

fn main() -> std::io::Result<()> {
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
            _ if line.starts_with("go ") => writeln!(stdout, "bestmove e2e4")?,
            _ => {}
        }
        stdout.flush()?;
    }
    Ok(())
}
