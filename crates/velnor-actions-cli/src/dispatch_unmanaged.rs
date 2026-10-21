//! `velnor-actions unmanaged` dispatch: list `.github` entries generation keeps.

use std::process::ExitCode;

use velnor_actions_orchestrator::{resolve_root, unmanaged_report};

use crate::dispatch::{fail_public, working_dir};

/// Print one unmanaged path per line; fail only under `deny` with entries.
pub(crate) fn run_unmanaged() -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let report = resolve_root(&cwd).and_then(|root| unmanaged_report(&root));
    match report {
        Ok(report) => {
            for entry in &report.entries {
                println!("{entry}");
            }
            if report.is_denied() {
                eprintln!(
                    "velnor-actions: unmanaged_github_entries: [github] unmanaged = \"deny\""
                );
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}
