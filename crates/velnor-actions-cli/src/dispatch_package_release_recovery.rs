// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;

use velnor_actions_orchestrator::{OrchestratorError, recover_package_release_from_env};

pub(super) fn run() -> ExitCode {
    match recover_package_release_from_env() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&error),
    }
}

fn fail_internal(error: &OrchestratorError) -> ExitCode {
    let message = error.to_string().replace(['\n', '\r'], " ");
    eprintln!("velnor-actions: internal request failed: {message}");
    ExitCode::from(1)
}
