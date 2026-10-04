//! Closed nightly observer templates: no checkout or repository command surface.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{
    CompiledSourceHelper, Job, SourceBoundOperation, Step, StepKind, ToolCacheDomain, Trigger,
};

use crate::{RenderError, delivery_tools::DeliveryToolContext};

/// Fixed issue observer interpreter source.
pub const OBSERVER_SCRIPT: &str = include_str!("verification_observer.py");

/// Build the exact Bash source that embeds the fixed Python observer.
///
/// The source is executed through the compiled managed-tool recipe.  The
/// Python bytes are embedded in the source record so the job never accepts a
/// repository file, command vector, or mutable inline program.
/// # Errors
/// Rejects an invalid generator marker version.
pub fn observer_source(version: &str) -> Result<String, RenderError> {
    crate::marker::with_marker(version, &observer_source_body())
}

fn observer_source_body() -> String {
    format!(
        "set -euo pipefail\nexec python3 -I -S - <<'VELNOR_VERIFICATION_OBSERVER'\n{OBSERVER_SCRIPT}\nVELNOR_VERIFICATION_OBSERVER\n"
    )
}

/// Prepare only isolated catalog-pinned Python and Gh for the native observer.
/// # Errors
/// Rejects invalid tool identities or step argv.
pub fn observer_setup_steps(
    tools: &DeliveryToolContext,
    runs_on: &str,
) -> Result<Vec<Step>, RenderError> {
    tools.validate()?;
    let setup = crate::mise_setup_step(&tools.mise, ToolCacheDomain::Full, runs_on)?;
    let preparation = crate::source_helper::source_helper_step(
        "Prepare exact observer tools",
        &tools.preparation,
        tools.preparation.environment().clone(),
    )?;
    Ok(vec![setup, preparation])
}

/// Fixed helper binds issues to the generated repository and default branch.
/// # Errors
/// Rejects unsafe identities or argv before rendering.
pub fn observer_step(
    repository: &str,
    branch: &str,
    required_job: &str,
    record: &CompiledSourceHelper,
) -> Result<Step, RenderError> {
    crate::release_spec::validate_repository(repository)?;
    if !velnor_actions_contract::is_valid_branch_name(branch) {
        return Err(RenderError::InvalidWorkflow(
            "invalid_observer_branch".to_owned(),
        ));
    }
    if required_job != crate::render::FINAL_JOB_ID {
        return Err(RenderError::InvalidWorkflow(
            "observer_requires_Required".to_owned(),
        ));
    }
    validate_observer_record(record, repository, branch, required_job)?;
    crate::source_helper::source_helper_step(
        "Open or update nightly failure signal",
        record,
        record.environment().clone(),
    )
}

fn validate_observer_record(
    record: &CompiledSourceHelper,
    repository: &str,
    branch: &str,
    required_job: &str,
) -> Result<(), RenderError> {
    let reject = || RenderError::InvalidWorkflow("observer_payload_not_fixed".to_owned());
    let source_body = observer_source_body();
    if record.invocation().descriptor().operation() != SourceBoundOperation::VerificationObserver
        || !record.invocation().args().is_empty()
        || record.source().split_once('\n').map(|(_, body)| body) != Some(source_body.as_str())
    {
        return Err(reject());
    }
    let Some(recipe) = record.execution_recipe() else {
        return Err(reject());
    };
    if recipe.credential_scope() != NativeCredentialScope::GithubIssueWrite {
        return Err(reject());
    }
    let mut expected = BTreeMap::from([
        ("APPROVED_REPOSITORY".to_owned(), repository.to_owned()),
        (
            "GITHUB_REPOSITORY".to_owned(),
            "${{ github.repository }}".to_owned(),
        ),
        ("DEFAULT_BRANCH".to_owned(), branch.to_owned()),
        ("REF".to_owned(), "${{ github.ref }}".to_owned()),
        (
            "REF_PROTECTED".to_owned(),
            "${{ github.ref_protected }}".to_owned(),
        ),
        (
            "EVENT_NAME".to_owned(),
            "${{ github.event_name }}".to_owned(),
        ),
        ("SOURCE_SHA".to_owned(), "${{ github.sha }}".to_owned()),
        ("RUN_ID".to_owned(), "${{ github.run_id }}".to_owned()),
        (
            "RUN_ATTEMPT".to_owned(),
            "${{ github.run_attempt }}".to_owned(),
        ),
        (
            "REQUIRED_RESULT".to_owned(),
            format!("${{{{ needs.{required_job}.result }}}}"),
        ),
    ]);
    expected.extend(recipe.environment().clone());
    if record.environment() != &expected {
        return Err(reject());
    }
    Ok(())
}

/// Simulation is an explicit failing Required step, never a passing substitute.
/// # Errors
/// Rejects an invalid fixed shell vector.
pub fn simulation_step() -> Result<Step, RenderError> {
    let mut step = crate::shell_step(
        "Simulate verification failure",
        vec!["false".to_owned()],
        BTreeMap::new(),
    )?;
    step.condition = Some(
        "github.event_name == 'workflow_dispatch' && toJSON(inputs.simulate_failure) == 'true'"
            .to_owned(),
    );
    Ok(step)
}

/// Reject mutable observer or simulation payloads before granting native token use.
/// The observer steps must resolve to compiled records in `records`; no
/// action metadata or serialized setup is reconstructed here.
/// # Errors
/// Only the closed templates generated here are allowed.
pub(crate) fn validate_observer_jobs(
    jobs: &BTreeMap<String, Job>,
    triggers: &Trigger,
    records: &[CompiledSourceHelper],
    version: &str,
) -> Result<(), RenderError> {
    crate::source_helper::validate_registry(records, version)?;
    if let Some(job) = jobs.get("verification-observer") {
        let reject = || RenderError::InvalidWorkflow("observer_payload_not_fixed".to_owned());
        if job.steps.len() != 3 {
            return Err(reject());
        }
        let [acquire, preparation, observer] = job.steps.as_slice() else {
            return Err(reject());
        };
        if !registry_step("Acquire qualified Mise", acquire, records, |record| {
            record.invocation().descriptor().operation() == SourceBoundOperation::MiseBootstrap
                && record
                    .environment()
                    .get("MISE_DATA_DIR")
                    .map(String::as_str)
                    == Some(ToolCacheDomain::Full.root())
                && record
                    .environment()
                    .get("VELNOR_MISE_TARGET")
                    .map(String::as_str)
                    == velnor_actions_contract::tool_target_for_runner_label(&job.runs_on)
        }) {
            return Err(reject());
        }
        if !registry_step(
            "Prepare exact observer tools",
            preparation,
            records,
            |record| {
                record.invocation().descriptor().operation()
                    == SourceBoundOperation::MiseToolPrepare
                    && !record.invocation().installed_selectors().is_empty()
            },
        ) {
            return Err(reject());
        }
        let StepKind::SourceBoundHelper { .. } = &observer.kind else {
            return Err(reject());
        };
        let Some(repository) = observer_env(observer, "APPROVED_REPOSITORY") else {
            return Err(reject());
        };
        let Some(branch) = observer_env(observer, "DEFAULT_BRANCH") else {
            return Err(reject());
        };
        let Some(record) = records.iter().find(|record| {
            record.invocation().descriptor().operation()
                == SourceBoundOperation::VerificationObserver
                && record.invocation()
                    == match &observer.kind {
                        StepKind::SourceBoundHelper { invocation, .. } => invocation,
                        _ => unreachable!(),
                    }
                && record.environment()
                    == match &observer.kind {
                        StepKind::SourceBoundHelper { env, .. } => env,
                        _ => unreachable!(),
                    }
        }) else {
            return Err(reject());
        };
        observer_step(repository, branch, crate::render::FINAL_JOB_ID, record).map_or_else(
            |_| Err(reject()),
            |expected| (observer == &expected).then_some(()).ok_or_else(reject),
        )?;
    }
    validate_simulation(jobs, triggers)
}

fn registry_step(
    name: &str,
    step: &Step,
    records: &[CompiledSourceHelper],
    extra: impl Fn(&CompiledSourceHelper) -> bool,
) -> bool {
    records.iter().any(|record| {
        extra(record)
            && crate::source_helper::source_helper_step(name, record, record.environment().clone())
                .is_ok_and(|expected| &expected == step)
    })
}

fn observer_env<'a>(step: &'a Step, key: &str) -> Option<&'a str> {
    let StepKind::SourceBoundHelper { env, .. } = &step.kind else {
        return None;
    };
    env.get(key).map(String::as_str)
}

fn validate_simulation(
    jobs: &BTreeMap<String, velnor_actions_contract::Job>,
    triggers: &velnor_actions_contract::Trigger,
) -> Result<(), RenderError> {
    let reject = || RenderError::InvalidWorkflow("simulation_payload_not_fixed".to_owned());
    if jobs.contains_key("verification-simulation") {
        return Err(reject());
    }
    let expected = simulation_step()?;
    let configured =
        jobs.contains_key("verification-observer") && triggers.workflow_dispatch.is_some();
    let mut found = 0;
    for (id, job) in jobs {
        for (index, step) in job.steps.iter().enumerate() {
            if step.name != expected.name
                && !step
                    .condition
                    .as_ref()
                    .is_some_and(|value| value.contains("inputs.simulate_failure"))
            {
                continue;
            }
            if !configured || id != crate::render::PLAN_JOB_ID || step != &expected
                || job.steps.iter().position(|step| matches!(&step.kind, StepKind::Internal { operation } if operation == crate::steps::PLAN_OPERATION)).is_none_or(|plan_index| index + 1 != plan_index) {
                return Err(reject());
            }
            found += 1;
        }
    }
    if configured {
        let input = triggers
            .workflow_dispatch
            .as_ref()
            .and_then(|dispatch| {
                dispatch
                    .inputs
                    .iter()
                    .find(|input| input.name == "simulate_failure")
            })
            .ok_or_else(reject)?;
        if found != 1
            || jobs.get(crate::render::FINAL_JOB_ID).is_none_or(|job| {
                !job.needs
                    .iter()
                    .any(|need| need == crate::render::PLAN_JOB_ID)
            })
            || input.input_type != velnor_actions_contract::workflow::ir::DispatchInputType::Boolean
            || input.required
            || input.default.as_deref() != Some("false")
        {
            return Err(reject());
        }
    } else if triggers.workflow_dispatch.as_ref().is_some_and(|dispatch| {
        dispatch
            .inputs
            .iter()
            .any(|input| input.name == "simulate_failure")
    }) {
        return Err(reject());
    }
    Ok(())
}

#[cfg(test)]
#[path = "verification_observer_tests.rs"]
pub(super) mod tests;

#[cfg(test)]
#[path = "verification_observer/tests.rs"]
mod tests_contract;
