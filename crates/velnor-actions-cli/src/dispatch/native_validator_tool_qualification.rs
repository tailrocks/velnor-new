// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;

pub(super) fn run() -> ExitCode {
    super::unit_exit(
        velnor_actions_orchestrator::run_native_validator_tool_qualification_from_env(concat!(
            "velnor-actions ",
            env!("CARGO_PKG_VERSION")
        )),
    )
}
