//! The `capabilities` command: a read-only platform probe.

use super::*;

pub(crate) fn run_capabilities(machine: bool) -> ExitCode {
    let report = capabilities::probe();
    if machine {
        print_json(&MachineOutput::Capabilities { report });
    } else {
        capabilities::print_text(&report);
    }
    ExitCode::SUCCESS
}
