// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use velnor_actions_contract::workflow::step_identity::{
    TOOL_SEED_USES, TOOLS_CACHE_PRELUDE_USES, TOOLS_CACHE_RESTORE_USES,
};
use velnor_actions_contract::workflow::{
    JACTIONLINT_VERIFIER_USES, RUST_VERIFIER_USES, Step, StepKind, StepRole,
};

pub(crate) const TOFU_PROVIDER_PRELUDE_ACTION_PREFIX: &str =
    "./.github/actions/tofu-provider-prelude-";
pub(crate) const DECLARED_TASK_ACTION_PREFIX: &str = "./.github/actions/declared-task-";

pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    if RUST_VERIFIER_USES.contains(&uses)
        || uses == JACTIONLINT_VERIFIER_USES
        || matches!(
            uses,
            TOOL_SEED_USES
                | TOOLS_CACHE_RESTORE_USES
                | crate::tofu_cache::TOFU_PROVIDER_ADMISSION_USES
        )
        || TOOLS_CACHE_PRELUDE_USES.contains(&uses)
        || is_generated_provider_prelude(uses)
        || is_crate_obligation_action(uses)
        || is_generated_declared_task(uses)
    {
        return Ok(());
    }
    let Some((name, sha)) = uses.split_once('@') else {
        return Err(RenderError::BadActionRef(format!("missing_sha:{uses}")));
    };
    let Some((owner, repo)) = name.split_once('/') else {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    };
    if owner.is_empty() || repo.is_empty() || !is_action_name(name) {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    }
    if name.starts_with("actions/setup-") || name == "taiki-e/install-action" {
        return Err(RenderError::BadActionRef(format!(
            "forbidden_action:{uses}"
        )));
    }
    if !velnor_actions_contract::ids::is_lower_hex_len(sha, 40) {
        return Err(RenderError::BadActionRef(format!("unpinned_ref:{uses}")));
    }
    Ok(())
}

pub(crate) const CRATE_OBLIGATION_ACTION_PREFIX: &str = "./.github/actions/crate-obligation-";

pub(crate) fn is_crate_obligation_action(uses: &str) -> bool {
    let Some(id) = uses.strip_prefix(CRATE_OBLIGATION_ACTION_PREFIX) else {
        return false;
    };
    !id.is_empty()
        && velnor_actions_contract::is_crate_job_id(id)
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

pub(crate) fn is_generated_provider_prelude(uses: &str) -> bool {
    uses.strip_prefix(TOFU_PROVIDER_PRELUDE_ACTION_PREFIX)
        .is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix.bytes().all(|byte| byte.is_ascii_digit())
                && suffix
                    .parse::<usize>()
                    .is_ok_and(|index| index.to_string() == suffix)
        })
}

pub(crate) fn is_generated_declared_task(uses: &str) -> bool {
    uses.strip_prefix(DECLARED_TASK_ACTION_PREFIX)
        .is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix.bytes().all(|byte| byte.is_ascii_digit())
                && suffix
                    .parse::<usize>()
                    .is_ok_and(|index| index.to_string() == suffix)
        })
}

pub(crate) fn validate_verifier_call(
    step: &Step,
    uses: &str,
    with: &std::collections::BTreeMap<String, String>,
    env: &std::collections::BTreeMap<String, String>,
) -> Result<(), RenderError> {
    let expected_role = if RUST_VERIFIER_USES.contains(&uses) {
        Some(StepRole::VerifyRustManifest)
    } else if uses == JACTIONLINT_VERIFIER_USES {
        Some(StepRole::VerifyJactionlint)
    } else {
        None
    };
    let verifier_role = matches!(
        step.role,
        Some(StepRole::VerifyRustManifest | StepRole::VerifyJactionlint)
    );
    if expected_role.is_none() && !verifier_role {
        return Ok(());
    }
    if expected_role != step.role || !with.is_empty() || !env.is_empty() {
        return Err(RenderError::InvalidWorkflow(
            "verifier_action_identity_mismatch".to_owned(),
        ));
    }
    if !matches!(&step.kind, StepKind::Action { .. }) {
        return Err(RenderError::InvalidWorkflow(
            "verifier_action_payload_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn is_action_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
}
