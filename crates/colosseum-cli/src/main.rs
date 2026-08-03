//! Independent headless executable for Colosseum CLI.

use std::process::ExitCode;

fn main() -> ExitCode {
    colosseum_cli::run()
}
