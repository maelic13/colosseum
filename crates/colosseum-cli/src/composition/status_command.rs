//! The `status` command: the official state of any run, unmodified.

use super::*;

pub(crate) fn run_status(run_directory: &Path, machine: bool) -> ExitCode {
    let record = match RunRecord::read(run_directory) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("status failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if machine {
        print_json(&MachineOutput::RunStatus {
            run_directory,
            record,
        });
    } else {
        println!("command: {}", record.command);
        println!("status: {:?}", record.status);
        println!("config: {}", record.config_sha256);
        println!(
            "committed units: {}",
            record.official_sample.committed_units
        );
        println!("scored games: {}", record.official_sample.scored_games);
        println!("anomalies: {}", record.anomalies.len());
        // The last block the run published, verbatim: a closed console loses
        // nothing, and `status` never invents a second account of the run.
        match &record.progress {
            Some(block) => print!("{}", block.render()),
            None => println!("progress: none published yet"),
        }
    }
    ExitCode::SUCCESS
}
