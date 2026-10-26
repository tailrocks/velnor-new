// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{GeneratorLock, Step};

use crate::OrchestratorError;
use crate::pins::{lock_acquire_for_runner, lock_acquire_step};
use velnor_actions_workflow_renderer::steps::STAGED_BINARY_PREFIX;

pub(crate) fn published_lock_acquire_step(
    lock: &GeneratorLock,
    label: &str,
    version: &str,
) -> Result<Step, OrchestratorError> {
    if version != lock.generator.version {
        return Err(OrchestratorError::Contract {
            problem: "published_lock_helper_version_mismatch".to_owned(),
        });
    }
    lock_acquire_step(lock, label, &format!("{STAGED_BINARY_PREFIX}{version}"))
}

pub(crate) fn published_lock_acquire_for_runner(
    lock: &GeneratorLock,
    runner: &velnor_actions_contract::config::CheckRunner,
    version: &str,
) -> Result<Step, OrchestratorError> {
    if version != lock.generator.version {
        return Err(OrchestratorError::Contract {
            problem: "published_lock_helper_version_mismatch".to_owned(),
        });
    }
    lock_acquire_for_runner(lock, runner, &format!("{STAGED_BINARY_PREFIX}{version}"))
}
