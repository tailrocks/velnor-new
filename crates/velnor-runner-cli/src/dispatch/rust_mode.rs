// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::ExitCode;

use velnor_runner_host::{
    CreatedSetDeletion, RustRunnerLauncher, detach_rust_runner_connection,
    run_rust_runner_doctor_probe, wait_for_rust_runner_connection_drain,
    wait_for_rust_runner_drain,
};

use super::disconnect::disconnect_with_detach;
use super::drain::drain_with;

pub(super) fn drain(state: &Path) -> ExitCode {
    drain_with(state, true, wait_for_rust_runner_drain)
}

pub(super) fn disconnect(state: &Path, connection_id: &str, wait: bool) -> ExitCode {
    disconnect_with_detach(
        state,
        connection_id,
        wait,
        wait_for_rust_runner_connection_drain,
        detach_rust_runner_connection,
        |_, _, _| Ok(CreatedSetDeletion::Deleted),
    )
}

pub(super) fn doctor(state: &Path) -> ExitCode {
    match run_rust_runner_doctor_probe(state, &RustRunnerLauncher::from_environment()) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("doctor probe failed ({}): {error}", error.code());
            ExitCode::from(1)
        }
    }
}
