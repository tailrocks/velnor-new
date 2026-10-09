//! Fixed rendering and policy for isolated verification-only task jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_config::{
    RunsOn, VERIFICATION_TASK_JOB_PREFIX, VerificationRunner, VerificationTask,
};
use velnor_actions_contract_workflow::{
    HOSTED_SUFFIX, Job, JobTimeout, PermissionLevel, Permissions, SCALE_SUFFIX, Step,
};

use crate::context::PLAN_JOB_ID;
use velnor_actions_workflow_steps::{MiseSetup, RenderError, mise_setup_step, shell_step, steps};

#[cfg(test)]
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
    /// Typed helper-staging steps attached before output collection.
    /// Empty-output tasks keep this empty. Velnor policy fills this from
    /// its lock or pre-seed attach before final job validation.
    pub staging_steps: Vec<Step>,
}

/// Step installing the repo's locked task tool closure.
pub const INSTALL_VERIFICATION_TOOLS_NAME: &str = "Install locked task tools";
/// Step running the declared, credential-scrubbed Mise task.
pub const RUN_VERIFICATION_TASK_NAME: &str = "Run declared Mise task";

impl VerificationTaskPolicy {
    /// Generated GitHub job key for this declaration.
    #[must_use]
    pub fn job_id(&self) -> String {
        format!("{VERIFICATION_TASK_JOB_PREFIX}{}", self.task.id)
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
    let needs_plan = !policy.task.outputs.is_empty();
    let steps = verification_steps(policy, checkout_uses)?;
    Ok(Job {
        outputs: Vec::new(),
        display_name: format!("Verify {}", policy.task.id),
        runs_on: policy.runner_label.clone(),
        check_runner: None,
        timeout_minutes: timeout,
        needs: if needs_plan {
            vec![PLAN_JOB_ID.to_owned()]
        } else {
            Vec::new()
        },
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
        if !policy.task.outputs.is_empty() && policy.staging_steps.is_empty() {
            return Err(RenderError::InvalidWorkflow(format!(
                "verification_task_staging_missing:{}",
                policy.task.id
            )));
        }
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
    if jobs.keys().any(|id| {
        id.starts_with(VERIFICATION_TASK_JOB_PREFIX)
            && !job_ids.iter().any(|expected| expected == id)
    }) {
        return Err(RenderError::InvalidWorkflow(
            "undeclared_verification_job".to_owned(),
        ));
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

/// Add every verification job to the required-check fan-in.
/// # Errors
pub(crate) fn extend_required_needs(
    jobs: &mut BTreeMap<String, Job>,
    verification_ids: &[String],
) -> Result<(), RenderError> {
    if verification_ids.is_empty() {
        return Ok(());
    }
    let required = jobs.get_mut(crate::context::FINAL_JOB_ID).ok_or_else(|| {
        RenderError::InvalidWorkflow("verification_tasks_require_required_job".to_owned())
    })?;
    for id in verification_ids {
        if !required.needs.contains(id) {
            required.needs.push(id.clone());
        }
    }
    Ok(())
}

/// Build the exact step sequence, with credentials absent before Mise reads config.
fn verification_steps(
    policy: &VerificationTaskPolicy,
    checkout_uses: &str,
) -> Result<Vec<Step>, RenderError> {
    let mut task_steps = vec![steps::checkout_step(checkout_uses)?];
    task_steps.extend(policy.staging_steps.iter().cloned());
    if !policy.task.outputs.is_empty() {
        task_steps.push(crate::closure::download_plan_step()?);
    }
    task_steps.extend([
        mise_setup_step(&policy.mise_setup)?,
        shell_step(
            INSTALL_VERIFICATION_TOOLS_NAME,
            vec![
                "mise".to_owned(),
                "install".to_owned(),
                "--locked".to_owned(),
            ],
            BTreeMap::new(),
        )?,
        shell_step(
            RUN_VERIFICATION_TASK_NAME,
            vec![
                "mise".to_owned(),
                "run".to_owned(),
                policy.task.mise_task.clone(),
            ],
            BTreeMap::new(),
        )?,
    ]);
    if !policy.task.outputs.is_empty() {
        task_steps.push(steps::verification_artifact_export_step(&policy.task.id));
        task_steps.push(steps::verification_artifact_upload_step()?);
    }
    Ok(task_steps)
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
    policy.mise_setup.validate()
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
