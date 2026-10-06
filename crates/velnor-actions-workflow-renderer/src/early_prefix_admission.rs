//! Closed generated consumer prefix; arbitrary executable steps have no authority.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, Job, SourceBoundOperation, Step, StepKind, ToolCacheDomain,
};

use crate::{RenderError, steps};

/// Canonical acquisition command, shared by generation and prefix admission.
/// # Errors
/// Rejects paths that can escape the fixed shell template.
pub fn acquisition_argv(staged: &str) -> Result<Vec<String>, RenderError> {
    let suffix = staged
        .strip_prefix(steps::STAGED_BINARY_PREFIX)
        .ok_or_else(|| invalid("early_acquire_staged_path"))?;
    if suffix.is_empty()
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(invalid("early_acquire_staged_path"));
    }
    let (dir, _) = staged
        .rsplit_once('/')
        .ok_or_else(|| invalid("early_acquire_staged_path"))?;
    let script = format!(
        "mkdir -p \"{dir}\" && curl -fsSL --proto '=https' --tlsv1.2 \"$VELNOR_ASSET_URL\" -o \"{staged}\" && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x \"{staged}\""
    );
    Ok(vec!["sh".to_owned(), "-c".to_owned(), script])
}

/// Compare the complete finite prefix against compiled owner constructions.
/// # Errors
/// Rejects extra operations, altered control steps and unbound planning helpers.
pub(crate) fn validate(
    job: &Job,
    staged: &str,
    checkout_uses: &str,
    platform: &Step,
    restore: &Step,
    bootstrap: &Step,
    records: &[CompiledSourceHelper],
) -> Result<(), RenderError> {
    let expected_early = crate::early_plan::early_plan_step()?;
    let boundary = job
        .steps
        .iter()
        .position(|step| step == &expected_early)
        .ok_or_else(|| invalid("early_prefix_missing_boundary"))?;
    let prefix = &job.steps[..=boundary];
    let [
        checkout,
        acquire,
        actual_platform,
        actual_restore,
        actual_bootstrap,
        prepare,
        request,
        early,
    ] = prefix
    else {
        return Err(invalid("early_prefix_unowned_operation"));
    };
    let mut expected_checkout = steps::checkout_step(checkout_uses)?;
    if let StepKind::Action { with, .. } = &mut expected_checkout.kind {
        with.insert("fetch-depth".to_owned(), "0".to_owned());
    }
    if checkout != &expected_checkout
        || actual_platform != platform
        || actual_restore != restore
        || actual_bootstrap != bootstrap
        || request != &steps::write_request_step(steps::PLAN_OPERATION)?
        || early != &crate::early_plan::early_plan_step()?
    {
        return Err(invalid("early_prefix_owner_step_changed"));
    }
    validate_acquire(acquire, staged)?;
    validate_prepare(prepare, &job.runs_on, records)
}

fn validate_acquire(step: &Step, staged: &str) -> Result<(), RenderError> {
    let StepKind::Shell { env, .. } = &step.kind else {
        return Err(invalid("early_prefix_acquire_kind"));
    };
    let keys = [
        steps::ASSET_SHA_ENV,
        steps::ASSET_URL_ENV,
        steps::RELEASE_COMMIT_ENV,
    ];
    let asset_env: BTreeMap<_, _> = keys
        .into_iter()
        .map(|key| {
            env.get(key)
                .cloned()
                .map(|value| (key.to_owned(), value))
                .ok_or_else(|| invalid("early_prefix_acquire_environment"))
        })
        .collect::<Result<_, _>>()?;
    let mut expected = steps::acquire_velnor_step(staged, &asset_env)?;
    if let StepKind::Shell { env, .. } = &mut expected.kind {
        env.insert(
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::Planning.root().to_owned(),
        );
    }
    if step != &expected {
        return Err(invalid("early_prefix_acquire_changed"));
    }
    Ok(())
}

fn validate_prepare(
    step: &Step,
    runs_on: &str,
    records: &[CompiledSourceHelper],
) -> Result<(), RenderError> {
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        return Err(invalid("early_prefix_prepare_not_source_bound"));
    };
    let target = velnor_actions_contract::tool_target_for_runner_label(runs_on)
        .ok_or_else(|| invalid("early_prefix_runner"))?;
    if invocation.descriptor().operation() != SourceBoundOperation::MiseToolPrepare
        || invocation.args().first().map(String::as_str) != Some("planning")
        || invocation.args().get(1).map(String::as_str) != Some(target)
        || env.get("MISE_DATA_DIR").map(String::as_str) != Some(ToolCacheDomain::Planning.root())
        || !invocation.execution_prefix().is_empty()
        || !planning_footprint(invocation.installed_selectors())
    {
        return Err(invalid("early_prefix_prepare_authority"));
    }
    let matching: Vec<_> = records
        .iter()
        .filter(|record| record.invocation() == invocation && record.environment() == env)
        .collect();
    let [record] = matching.as_slice() else {
        return Err(invalid("early_prefix_prepare_unregistered"));
    };
    let expected =
        crate::source_helper::source_helper_step("Prepare planning tools", record, env.clone())?;
    if step != &expected {
        return Err(invalid("early_prefix_prepare_changed"));
    }
    Ok(())
}

fn planning_footprint(selectors: &[String]) -> bool {
    if selectors.len() != 4
        || selectors.iter().any(|selector| {
            selector
                .split_once('@')
                .is_none_or(|(_, version)| version.is_empty())
        })
    {
        return false;
    }
    let names: Vec<_> = selectors
        .iter()
        .filter_map(|selector| selector.split_once('@').map(|(name, _)| name))
        .collect();
    names == ["actionlint", "gh", "shellcheck", "zizmor"]
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(reason.to_owned())
}

#[cfg(test)]
#[path = "early_prefix_admission_tests.rs"]
mod tests;
