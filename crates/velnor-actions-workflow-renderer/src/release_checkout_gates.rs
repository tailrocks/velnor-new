// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::release_gates::{GIT_TOKEN_ENV, GIT_TOKEN_REF};
use crate::{RenderError, ambient_shell_step, release_jobs::ReleaseRole};
use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepKind, StepRole};

pub fn policy_checkout(version: &str) -> Result<Step, RenderError> {
    crate::marker::validate_version(version)?;
    ambient_shell_step(
        "Checkout",
        vec![
            format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{version}"),
            "crate-release".to_owned(),
            "checkout-policy".to_owned(),
        ],
        BTreeMap::from([(GIT_TOKEN_ENV.to_owned(), GIT_TOKEN_REF.to_owned())]),
    )
}

pub fn source_checkout(version: &str, sha: &str) -> Result<Step, RenderError> {
    crate::marker::validate_version(version)?;
    ambient_shell_step(
        "Checkout exact source",
        vec![
            format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{version}"),
            "crate-release".to_owned(),
            "checkout-source".to_owned(),
        ],
        BTreeMap::from([
            (GIT_TOKEN_ENV.to_owned(), GIT_TOKEN_REF.to_owned()),
            ("VELNOR_SOURCE_SHA".to_owned(), sha.to_owned()),
        ]),
    )
}

pub fn check_checkout_shape(
    id: &str,
    steps: &[Step],
    role: ReleaseRole,
    sha: &str,
    version: &str,
) -> Result<(), RenderError> {
    let invalid = || RenderError::InvalidWorkflow(format!("release_checkout_invalid:{id}"));
    if steps.iter().any(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")))
        || steps.first().is_none_or(|step| step.role != Some(StepRole::AcquireVelnor))
        || steps.get(1) != Some(&policy_checkout(version)?)
        || steps.iter().filter(|step| step.name == "Checkout").count() != 1
    { return Err(invalid()); }
    let sources: Vec<_> = steps
        .iter()
        .filter(|step| step.name == "Checkout exact source")
        .collect();
    if role == ReleaseRole::Preparation {
        if !sources.is_empty() {
            return Err(invalid());
        }
    } else {
        let mut expected = source_checkout(version, sha)?;
        expected.condition = Some("steps.release-admission.outputs.eligible == 'true'".to_owned());
        if sources != [&expected] {
            return Err(invalid());
        }
    }
    Ok(())
}
