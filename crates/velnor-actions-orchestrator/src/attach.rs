// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{GeneratorLock, Step};

use crate::OrchestratorError;
use crate::planner::pins::{lock_acquire_for_runner, lock_acquire_step};
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
    if let Some(preview) = &lock.source_preview {
        if version != preview.receipt.version
            || runner.platform.release_target()
                != velnor_actions_contract::ReleaseTarget::LinuxX86_64
        {
            return Err(OrchestratorError::Contract {
                problem: "preview_controller_runner_or_version_unsupported".to_owned(),
            });
        }
        return velnor_actions_workflow_renderer::steps::protected_preview_acquire_step()
            .map_err(OrchestratorError::from);
    }
    if version != lock.generator.version {
        return Err(OrchestratorError::Contract {
            problem: "published_lock_helper_version_mismatch".to_owned(),
        });
    }
    lock_acquire_for_runner(lock, runner, &format!("{STAGED_BINARY_PREFIX}{version}"))
}
