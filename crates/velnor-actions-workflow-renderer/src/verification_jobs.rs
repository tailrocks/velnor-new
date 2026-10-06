//! Fixed rendering and policy for isolated verification-only task jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    HOSTED_SUFFIX, Job, JobTimeout, PermissionLevel, Permissions, RunsOn, SCALE_SUFFIX, Step,
    VerificationRunner, VerificationTask, WORKFLOW_TASK_JOB_PREFIX,
};

use crate::{MiseSetup, RenderError};

// Workflow-task job builders live beside the verification job builder
// (`#[path]`, no `lib.rs` edit).
#[path = "build_task_jobs.rs"]
pub(crate) mod build_task_jobs;
#[path = "build_task_mise.rs"]
pub(crate) mod build_task_mise;
#[path = "build_task_mise_tools.rs"]
pub(crate) mod build_task_mise_tools;
#[path = "native_image_jobs.rs"]
pub(crate) mod native_image_jobs;
#[path = "task_script.rs"]
pub(crate) mod task_script;
#[path = "verification_task_mise.rs"]
pub(crate) mod verification_task_mise;
#[path = "workflow_task_jobs.rs"]
pub(crate) mod workflow_task_jobs;

pub use build_task_jobs::{
    BuildTaskArtifact, BuildTaskPolicy, BuildTaskTool, build_build_task_job,
};
pub use native_image_jobs::{NativeImageTaskPolicy, build_native_image_job};
pub use workflow_task_jobs::WorkflowTaskPolicy;

#[cfg(test)]
#[path = "verification_jobs_tests.rs"]
mod tests;

/// Per-task, orchestrator-resolved runner and Mise binary pin.
#[derive(Debug, Clone)]
pub struct VerificationTaskPolicy {
    /// Validated task declaration from `.velnor/config.toml`.
    pub task: VerificationTask,
    /// Exact OS/architecture-specific runner label.
    pub runner_label: String,
    /// Configured Scale Set selector token, absent for schema 1.
    pub scale_set_token: Option<String>,
    /// Mise setup action with the binary digest for this runner target.
    pub mise_setup: MiseSetup,
    /// Per-task explicitly declared tools, resolved to locked prebuilt rows.
    pub selected_tools: Vec<build_task_jobs::BuildTaskTool>,
    /// SHA-256 of the source task config, if present.
    pub mise_config_sha256: Option<String>,
    /// SHA-256 of the source Mise lock, if present.
    pub mise_lock_sha256: Option<String>,
    /// SHA-256 of the source Rust toolchain file, if present.
    pub rust_toolchain_sha256: Option<String>,
}

/// Step installing only the declared task's locked prebuilt tool closure.
pub const INSTALL_VERIFICATION_TOOLS_NAME: &str = "Install declared prebuilt task tools";
/// Step running the declared, credential-scrubbed Mise task.
pub const RUN_VERIFICATION_TASK_NAME: &str = "Run declared Mise task";

impl VerificationTaskPolicy {
    /// Generated GitHub job key for this declaration.
    #[must_use]
    pub fn job_id(&self) -> String {
        format!("{WORKFLOW_TASK_JOB_PREFIX}{}", self.task.id)
    }

    /// Whether `job_id` is this declaration's base or schema-2 lane copy.
    #[must_use]
    pub fn owns_job_id(&self, job_id: &str) -> bool {
        let base = self.job_id();
        job_id == base
            || job_id == format!("{base}{HOSTED_SUFFIX}")
            || job_id == format!("{base}{SCALE_SUFFIX}")
    }
}

/// Build the exact read-only-token verification job for one declaration.
///
/// The shared renderer removes runner credentials before both direct Mise
/// steps: locked install reads repo config/hooks, and `run` executes the task.
/// # Errors
pub fn build_verification_task_job(
    policy: &VerificationTaskPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    validate_policy(policy)?;
    let timeout = JobTimeout::new(policy.task.timeout_minutes).map_err(RenderError::Contract)?;
    let steps = verification_steps(policy, checkout_uses)?;
    Ok(Job {
        display_name: format!("Verify {}", policy.task.id),
        runs_on: policy.runner_label.clone(),
        check_runner: None,
        timeout_minutes: timeout,
        needs: Vec::new(),
        condition: None,
        permissions: Some(verification_permissions()),
        environment: None,
        steps,
    })
}

/// Validate every task job and return its sorted job IDs.
/// # Errors
pub(crate) fn validate_verification_jobs(
    jobs: &BTreeMap<String, Job>,
    policies: &[VerificationTaskPolicy],
    checkout_uses: &str,
) -> Result<Vec<String>, RenderError> {
    let mut ids = BTreeSet::new();
    let mut last_id = None;
    let mut job_ids = Vec::with_capacity(policies.len());
    for policy in policies {
        validate_policy(policy)?;
        if last_id.is_some_and(|previous: &str| previous >= policy.task.id.as_str())
            || !ids.insert(policy.task.id.as_str())
        {
            return Err(RenderError::InvalidWorkflow(
                "verification_tasks_not_sorted_unique".to_owned(),
            ));
        }
        let base = policy.job_id();
        let variants = [
            (base.clone(), VerificationJobVariant::Base),
            (
                format!("{base}{HOSTED_SUFFIX}"),
                VerificationJobVariant::Hosted,
            ),
            (
                format!("{base}{SCALE_SUFFIX}"),
                VerificationJobVariant::ScaleSet,
            ),
        ];
        let mut found = false;
        for (id, variant) in variants {
            let Some(actual) = jobs.get(&id) else {
                continue;
            };
            found = true;
            let expected = expected_variant_job(policy, variant, actual, checkout_uses)?;
            if !same_job_contract(actual, &expected) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "verification_job_contract:{id}"
                )));
            }
            job_ids.push(id);
        }
        if !found {
            return Err(RenderError::InvalidWorkflow(format!(
                "verification_job_missing:{base}"
            )));
        }
        let hosted_id = format!("{base}{HOSTED_SUFFIX}");
        let scale_id = format!("{base}{SCALE_SUFFIX}");
        let has_hosted = jobs.contains_key(&hosted_id);
        let has_scale = jobs.contains_key(&scale_id);
        if has_hosted != has_scale || (has_hosted && jobs.contains_key(&base)) {
            return Err(RenderError::InvalidWorkflow(format!(
                "verification_task_lane_pair_incomplete:{base}"
            )));
        }
        last_id = Some(policy.task.id.as_str());
    }
    Ok(job_ids)
}

/// One rendered form of a typed verification declaration.
#[derive(Debug, Clone, Copy)]
enum VerificationJobVariant {
    Base,
    Hosted,
    ScaleSet,
}

/// Rebuild the fixed task job for one schema-2 placement.
fn expected_variant_job(
    policy: &VerificationTaskPolicy,
    variant: VerificationJobVariant,
    actual: &Job,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    let mut expected = build_verification_task_job(policy, checkout_uses)?;
    match variant {
        VerificationJobVariant::Base if actual.runs_on == policy.runner_label => {}
        VerificationJobVariant::Base if is_scale_set(&actual.runs_on) => {
            require_scale_set(policy, &actual.runs_on)?;
            expected.runs_on.clone_from(&actual.runs_on);
            expected.display_name.push_str(SCALE_TASK_NAME_SUFFIX);
        }
        VerificationJobVariant::Base => return Err(runner_mismatch(policy)),
        VerificationJobVariant::Hosted => {
            require_linux_task(policy)?;
            if actual.runs_on != policy.runner_label {
                return Err(runner_mismatch(policy));
            }
            expected.display_name.push_str(HOSTED_TASK_NAME_SUFFIX);
        }
        VerificationJobVariant::ScaleSet => {
            require_scale_set(policy, &actual.runs_on)?;
            expected.runs_on.clone_from(&actual.runs_on);
            expected.display_name.push_str(SCALE_TASK_NAME_SUFFIX);
        }
    }
    Ok(expected)
}

/// Lane suffixes mirror the schema-2 workflow contract.
const HOSTED_TASK_NAME_SUFFIX: &str = " / GitHub hosted / Linux x64";
const SCALE_TASK_NAME_SUFFIX: &str = " / Velnor Scale Set / Linux x64";

/// Ensure a Scale Set copy is only built for the Linux-x64 task kind.
fn require_linux_task(policy: &VerificationTaskPolicy) -> Result<(), RenderError> {
    if policy.task.runner == VerificationRunner::LinuxX64 {
        return Ok(());
    }
    Err(runner_mismatch(policy))
}

/// Require the exact Scale Set selector resolved from execution config.
fn require_scale_set(policy: &VerificationTaskPolicy, actual: &str) -> Result<(), RenderError> {
    require_linux_task(policy)?;
    if policy.scale_set_token.as_deref() == Some(actual) {
        return Ok(());
    }
    Err(runner_mismatch(policy))
}

/// Whether a rendered `runs-on` value is a validated Scale Set token.
fn is_scale_set(runs_on: &str) -> bool {
    RunsOn::parse(runs_on).is_ok_and(|selector| selector.is_scale_set())
}

/// Mismatch diagnostic for an unsupported verification-task placement.
fn runner_mismatch(policy: &VerificationTaskPolicy) -> RenderError {
    RenderError::InvalidWorkflow(format!("verification_runner_mismatch:{}", policy.task.id))
}

/// Build the exact step sequence, with credentials absent before Mise reads config.
fn verification_steps(
    policy: &VerificationTaskPolicy,
    checkout_uses: &str,
) -> Result<Vec<Step>, RenderError> {
    crate::verification_jobs::verification_task_mise::steps(policy, checkout_uses)
}

/// Validate the task schema and runner-to-platform binding.
fn validate_policy(policy: &VerificationTaskPolicy) -> Result<(), RenderError> {
    policy
        .task
        .validate(".velnor/config.toml")
        .map_err(RenderError::Contract)?;
    if policy.runner_label != policy.task.runner.runs_on() {
        return Err(RenderError::InvalidWorkflow(format!(
            "verification_runner_mismatch:{}",
            policy.task.id
        )));
    }
    policy.mise_setup.validate()?;
    crate::verification_jobs::verification_task_mise::validate_policy(policy)
}

/// Exact least-privilege token scopes for isolated task jobs.
fn verification_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
    }
}

/// Compare every `Job` field and the full ordered step contract.
fn same_job_contract(actual: &Job, expected: &Job) -> bool {
    actual.display_name == expected.display_name
        && actual.runs_on == expected.runs_on
        && actual.timeout_minutes == expected.timeout_minutes
        && actual.needs == expected.needs
        && actual.condition == expected.condition
        && actual.permissions == expected.permissions
        && actual.environment == expected.environment
        && actual.steps == expected.steps
}
