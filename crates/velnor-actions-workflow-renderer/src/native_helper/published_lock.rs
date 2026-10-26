// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::GeneratorLock;

use super::ReleaseHelperError;

pub(super) fn validate_published_generator_lock(
    lock: &GeneratorLock,
) -> Result<(), ReleaseHelperError> {
    lock.validate(".velnor/generator.lock")
        .map_err(|err| ReleaseHelperError::Problem(err.to_string()))?;
    lock.check_supported_targets(".velnor/generator.lock")
        .map_err(|err| ReleaseHelperError::Problem(err.to_string()))?;
    velnor_actions_mise::catalog::lock::verify_lock_against_published_release(lock)
        .map_err(|err| ReleaseHelperError::Problem(err.to_string()))
}
