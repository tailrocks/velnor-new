// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MiseTaskBootstrapPolicy, VerificationTaskPolicy};
use crate::RenderError;

pub(crate) fn steps(
    policy: &VerificationTaskPolicy,
    checkout_uses: &str,
) -> Result<Vec<velnor_actions_contract::Step>, RenderError> {
    let bootstrap = MiseTaskBootstrapPolicy::from_verification(policy);
    super::mise_task_bootstrap::steps(&bootstrap, &policy.mise_setup, checkout_uses, false, false)
}

pub(crate) fn validate_policy(policy: &VerificationTaskPolicy) -> Result<(), RenderError> {
    let bootstrap = MiseTaskBootstrapPolicy::from_verification(policy);
    super::mise_task_bootstrap::validate_policy(&bootstrap)
}
