// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;
use crate::workflow::step::Step;
use crate::workflow::step_mbx_lifecycle::validate_mbx_cleanup_sequence;
use crate::workflow::step_protocol::validate_tofu_provider_sequence;
use std::collections::BTreeSet;

pub fn validate_step_sequence(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    validate_step_identities(steps, scope)?;
    validate_mbx_cleanup_sequence(steps, scope)?;
    validate_tofu_provider_sequence(steps, scope)
}

pub fn validate_step_identity_scope(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    validate_step_identities(steps, scope)
}

fn validate_step_identities(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    let mut ids = BTreeSet::new();
    for step in steps {
        step.validate(scope)?;
        if let Some(step_id) = step.id
            && !ids.insert(step_id)
        {
            return Err(ContractError::identity(
                "job.steps.id",
                format!("duplicate_step_id:{scope}:{}", step_id.as_str()),
            ));
        }
    }
    Ok(())
}
