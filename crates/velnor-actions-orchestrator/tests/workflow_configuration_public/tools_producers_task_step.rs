// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::workflow::cache_family::{CARGO_HOME_EXPR, RUSTUP_HOME_EXPR};
use velnor_actions_contract::workflow::crate_job::task_digest_for_execution;
use velnor_actions_contract::{Step, StepKind, matrix_key_for_id, task_id_for_internal};

pub(super) fn task_execution_step() -> Result<Step, Box<dyn Error>> {
    let toolchain_inputs = ToolchainInputs {
        tools: vec!["rust@1.89.0".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let argv: Vec<String> = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.89.0",
        "--",
        "cargo",
        "test",
        "--locked",
    ]
    .map(str::to_owned)
    .to_vec();
    let task_id = task_id_for_internal("lint", "dev")?;
    let task_digest =
        task_digest_for_execution(&task_id, &argv, &toolchain_id(&toolchain_inputs)?)?;
    let matrix_id = format!("stack:rust|task:{task_id}");
    let matrix_key = matrix_key_for_id(&matrix_id)?;
    let env = [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_RUSTUP_HOME", RUSTUP_HOME_EXPR),
        ("MISE_CARGO_HOME", CARGO_HOME_EXPR),
        ("RUSTUP_HOME", RUSTUP_HOME_EXPR),
        ("CARGO_HOME", CARGO_HOME_EXPR),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    Ok(Step {
        parallel: false,
        name: "Run task".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::TaskExecution {
            argv,
            env,
            task_id,
            task_digest,
            toolchain_inputs,
            matrix_id,
            matrix_key,
            report_helper_version: "0.1.57".to_owned(),
            matrix_max_parallel: None,
        },
    })
}
