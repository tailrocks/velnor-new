//! Closed permission boundary for the protected nightly issue observer.

use std::collections::BTreeMap;

use super::{
    Job, PermissionLevel, Permissions, REQUIRED_JOB_ID, SourceBoundOperation, StepKind, Trigger,
};
use crate::errors::ContractError;

/// Fixed observer job identity.
pub const OBSERVER_JOB_ID: &str = "verification-observer";

const MISE_SETUP_STEP_NAME: &str = "Acquire qualified Mise";
const OBSERVER_TOOLS_STEP_NAME: &str = "Prepare exact observer tools";
const OBSERVER_STEP_NAME: &str = "Open or update nightly failure signal";
const GITHUB_TOKEN_EXPRESSION: &str = "${{ github.token }}";

/// Protected native event condition, bound to literal origin and default branch.
#[must_use]
pub fn observer_condition(repository: &str, branch: &str) -> String {
    format!(
        "always() && github.repository == '{repository}' && github.ref == 'refs/heads/{branch}' && github.ref_protected == true && (github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && needs.{REQUIRED_JOB_ID}.result != 'success'"
    )
}

/// Validate the only job allowed to hold issues write permission.
///
/// Source bytes, execution recipes, and managed-tool selectors are validated by
/// the renderer against its compiled owner registry. This contract gate checks
/// the closed workflow shape and the observer's runtime identity bindings.
/// # Errors
/// Rejects any wider permission surface, identity, event gate, or step shape.
pub(super) fn validate_observer(
    id: &str,
    job: &Job,
    workflow: &Permissions,
    triggers: &Trigger,
) -> Result<(), ContractError> {
    let permissions = job.permissions.as_ref().unwrap_or(workflow);
    if permissions.issues != PermissionLevel::Write && id != OBSERVER_JOB_ID {
        return Ok(());
    }
    let reject =
        || ContractError::identity("job.permissions", format!("invalid_issues_observer:{id}"));
    let expected = Permissions {
        contents: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
        issues: PermissionLevel::Write,
        pages: PermissionLevel::None,
        attestations: PermissionLevel::None,
    };
    if id != OBSERVER_JOB_ID
        || job.display_name != "Nightly failure observer"
        || job.environment.as_deref() != Some("verification-alerts")
        || job.permissions.as_ref() != Some(&expected)
        || job.needs != [REQUIRED_JOB_ID]
        || job.steps.len() != 3
        || triggers.schedule.is_none() && triggers.workflow_dispatch.is_none()
    {
        return Err(reject());
    }
    let observer_env = validate_steps(job).map_err(|()| reject())?;
    validate_bindings(job, triggers, observer_env).map_err(|()| reject())
}

fn validate_steps(job: &Job) -> Result<&BTreeMap<String, String>, ()> {
    let [setup, preparation, observer] = job.steps.as_slice() else {
        return Err(());
    };
    if job
        .steps
        .iter()
        .any(|step| step.id.is_some() || step.condition.is_some())
        || setup.name != MISE_SETUP_STEP_NAME
        || preparation.name != OBSERVER_TOOLS_STEP_NAME
        || observer.name != OBSERVER_STEP_NAME
    {
        return Err(());
    }
    let StepKind::SourceBoundHelper {
        invocation: bootstrap,
        ..
    } = &setup.kind
    else {
        return Err(());
    };
    if bootstrap.descriptor().operation() != SourceBoundOperation::MiseBootstrap {
        return Err(());
    }
    let StepKind::SourceBoundHelper {
        invocation: tools, ..
    } = &preparation.kind
    else {
        return Err(());
    };
    if tools.descriptor().operation() != SourceBoundOperation::MiseToolPrepare {
        return Err(());
    }
    let StepKind::SourceBoundHelper {
        invocation: observer_invocation,
        env,
    } = &observer.kind
    else {
        return Err(());
    };
    if observer_invocation.descriptor().operation() != SourceBoundOperation::VerificationObserver
        || !observer_invocation.args().is_empty()
    {
        return Err(());
    }
    Ok(env)
}

fn validate_bindings(
    job: &Job,
    triggers: &Trigger,
    env: &BTreeMap<String, String>,
) -> Result<(), ()> {
    let repository = env.get("APPROVED_REPOSITORY").ok_or(())?;
    let branch = env.get("DEFAULT_BRANCH").ok_or(())?;
    if !valid_repository(repository)
        || !crate::is_valid_branch_name(branch)
        || triggers.push_branches != [branch.to_owned()]
        || job.condition.as_deref() != Some(observer_condition(repository, branch).as_str())
    {
        return Err(());
    }
    if env.get("GITHUB_REPOSITORY").map(String::as_str) != Some("${{ github.repository }}")
        || env.get("REF").map(String::as_str) != Some("${{ github.ref }}")
        || env.get("REF_PROTECTED").map(String::as_str) != Some("${{ github.ref_protected }}")
        || env.get("EVENT_NAME").map(String::as_str) != Some("${{ github.event_name }}")
        || env.get("SOURCE_SHA").map(String::as_str) != Some("${{ github.sha }}")
        || env.get("RUN_ID").map(String::as_str) != Some("${{ github.run_id }}")
        || env.get("RUN_ATTEMPT").map(String::as_str) != Some("${{ github.run_attempt }}")
        || env.get("GH_TOKEN").map(String::as_str) != Some(GITHUB_TOKEN_EXPRESSION)
        || env.contains_key("GITHUB_TOKEN")
    {
        return Err(());
    }
    let required_result = format!("${{{{ needs.{REQUIRED_JOB_ID}.result }}}}");
    if env.get("REQUIRED_RESULT").map(String::as_str) != Some(required_result.as_str())
        || env
            .iter()
            .any(|(key, value)| key != "GH_TOKEN" && value.contains(GITHUB_TOKEN_EXPRESSION))
    {
        return Err(());
    }
    Ok(())
}

fn valid_repository(repository: &str) -> bool {
    let parts: Vec<_> = repository.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| !part.is_empty())
        && repository
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

/// Fixed isolated native helper environment.
///
/// Delivery contexts reuse these neutral defaults when constructing SDK-owned
/// execution recipes. The observer validator does not reconstruct that recipe.
#[must_use]
pub fn isolation_env() -> BTreeMap<String, String> {
    [
        ("MISE_DATA_DIR", "${{ runner.temp }}/velnor/mise"),
        ("MISE_CONFIG_DIR", "${{ runner.temp }}/velnor/mise-config"),
        ("MISE_CACHE_DIR", "${{ runner.temp }}/velnor/mise-cache"),
        ("MISE_STATE_DIR", "${{ runner.temp }}/velnor/mise-state"),
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("PYTHONHOME", ""),
        ("PYTHONPATH", ""),
        ("PYTHONNOUSERSITE", "1"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}
