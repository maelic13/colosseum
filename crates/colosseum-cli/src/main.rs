//! Independent headless composition root for Colosseum CLI.

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "colosseum-cli",
    version,
    about = "Run reproducible UCI chess-engine tests and experiments",
    long_about = "A headless harness for inspecting, testing and comparing ordinary UCI chess-engine executables."
)]
struct Cli {}

fn main() {
    let _ = Cli::parse();
}
