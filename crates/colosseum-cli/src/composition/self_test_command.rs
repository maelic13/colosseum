//! The `self-test` command: this executable checking itself.

use super::*;

pub(crate) async fn run_self_test(machine: bool) -> ExitCode {
    let report = self_test::execute().await;
    let success = report.success;
    if machine {
        print_json(&MachineOutput::SelfTest { report });
    } else {
        for check in &report.checks {
            let status = if check.success { "PASS" } else { "FAIL" };
            println!("[{status}] {} — {}", check.name, check.detail);
        }
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
