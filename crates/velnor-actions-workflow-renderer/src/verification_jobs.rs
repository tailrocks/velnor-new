//! Fixed rendering and policy for isolated verification-only task jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, Permissions, Step, VerificationTask,
};

use crate::{MiseSetup, RenderError, mise_setup_step, shell_step, steps};

/// Per-task, orchestrator-resolved runner and Mise binary pin.
#[derive(Debug, Clone)]
pub struct VerificationTaskPolicy {
    /// Validated task declaration from `.velnor/config.toml`.
    pub task: VerificationTask,
    /// Exact OS/architecture-specific runner label.
    pub runner_label: String,
    /// Mise setup action with the binary digest for this runner target.
    pub mise_setup: MiseSetup,
}

/// Stable prefix for generated verification job IDs.
pub const VERIFICATION_JOB_PREFIX: &str = "task-";
/// Step installing the repo's locked task tool closure.
pub const INSTALL_VERIFICATION_TOOLS_NAME: &str = "Install locked task tools";
/// Step running the declared, credential-scrubbed Mise task.
pub const RUN_VERIFICATION_TASK_NAME: &str = "Run declared Mise task";

impl VerificationTaskPolicy {
    /// Generated GitHub job key for this declaration.
    #[must_use]
    pub fn job_id(&self) -> String {
        format!("{VERIFICATION_JOB_PREFIX}{}", self.task.id)
    }
}

/// Build the exact tokenless verification job for one declaration.
///
/// The only repository-controlled work runs in the final `mise run` step,
/// after both direct-exec Mise steps have removed runner credentials.
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
        let id = policy.job_id();
        let actual = jobs.get(&id).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("verification_job_missing:{id}"))
        })?;
        let expected = build_verification_task_job(policy, checkout_uses)?;
        if !same_job_contract(actual, &expected) {
            return Err(RenderError::InvalidWorkflow(format!(
                "verification_job_contract:{id}"
            )));
        }
        last_id = Some(policy.task.id.as_str());
        job_ids.push(id);
    }
    if jobs.keys().any(|id| {
        id.starts_with(VERIFICATION_JOB_PREFIX) && !job_ids.iter().any(|expected| expected == id)
    }) {
        return Err(RenderError::InvalidWorkflow(
            "undeclared_verification_job".to_owned(),
        ));
    }
    Ok(job_ids)
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
    let required = jobs.get_mut(crate::render::FINAL_JOB_ID).ok_or_else(|| {
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
    Ok(vec![
        steps::checkout_step(checkout_uses)?,
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
    ])
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{
        Job, JobTimeout, PermissionLevel, VerificationRunner, VerificationTask,
        VerificationTaskKind,
    };

    use super::{
        VerificationTaskPolicy, build_verification_task_job, extend_required_needs,
        validate_verification_jobs,
    };
    use crate::MiseSetup;

    const CHECKOUT: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";

    fn policy(id: &str, runner: VerificationRunner) -> VerificationTaskPolicy {
        VerificationTaskPolicy {
            task: VerificationTask {
                id: id.to_owned(),
                kind: VerificationTaskKind::Verification,
                mise_task: format!("lint-{id}"),
                runner,
                timeout_minutes: 10,
            },
            runner_label: runner.runs_on().to_owned(),
            mise_setup: MiseSetup {
                uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
                version: "2026.9.18".to_owned(),
                sha256: "a".repeat(64),
            },
        }
    }

    #[test]
    fn task_job_is_unconditional_cache_off_and_credential_scrubbed() {
        let linux = policy("construct-assets", VerificationRunner::LinuxX64);
        let job = build_verification_task_job(&linux, CHECKOUT).expect("fixed task job");
        assert_eq!(job.runs_on, "ubuntu-26.04");
        assert!(job.needs.is_empty());
        assert!(job.condition.is_none());
        assert_eq!(job.timeout_minutes.minutes(), 10);
        let permissions = job.permissions.expect("job permissions are explicit");
        assert_eq!(permissions.contents, PermissionLevel::Read);
        assert_eq!(permissions.actions, PermissionLevel::None);
        assert_eq!(permissions.pull_requests, PermissionLevel::None);
        assert_eq!(permissions.id_token, PermissionLevel::None);
        assert_eq!(job.steps.len(), 4);
        if let velnor_actions_contract::StepKind::Action { uses, with, env } = &job.steps[0].kind {
            assert_eq!(uses.as_str(), CHECKOUT);
            assert_eq!(
                with.get("persist-credentials").map(String::as_str),
                Some("false")
            );
            assert!(env.is_empty());
        } else {
            panic!("checkout must be a pinned action");
        }
        let mise = &job.steps[1];
        if let velnor_actions_contract::StepKind::Action { with, env, .. } = &mise.kind {
            assert!(env.is_empty());
            assert_eq!(with.get("install").map(String::as_str), Some("false"));
            assert_eq!(with.get("env").map(String::as_str), Some("false"));
            assert_eq!(with.get("cache").map(String::as_str), Some("false"));
            assert_eq!(with.get("cache_save").map(String::as_str), Some("false"));
        } else {
            panic!("Mise setup must be an action");
        }
        for step in &job.steps[2..] {
            if let velnor_actions_contract::StepKind::Shell { run, env } = &step.kind {
                let unset = crate::toolchain_env::with_env_unset_argv(&[]);
                assert!(run.starts_with(&unset));
                for variable in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST {
                    assert!(
                        env.get(variable).is_some_and(String::is_empty),
                        "{variable}"
                    );
                }
            } else {
                panic!("Mise task commands must be shell steps");
            }
        }
        if let velnor_actions_contract::StepKind::Shell { run, .. } = &job.steps[2].kind {
            assert!(run.ends_with(&[
                "mise".to_owned(),
                "install".to_owned(),
                "--locked".to_owned(),
            ]));
        }
        if let velnor_actions_contract::StepKind::Shell { run, .. } = &job.steps[3].kind {
            assert!(run.ends_with(&[
                "mise".to_owned(),
                "run".to_owned(),
                "lint-construct-assets".to_owned(),
            ]));
        }
    }

    #[test]
    fn mixed_linux_and_apple_arm_tasks_keep_distinct_runners() {
        let linux = policy("linux-lint", VerificationRunner::LinuxX64);
        let macos = policy("native-format", VerificationRunner::MacosArm64);
        let linux_job = build_verification_task_job(&linux, CHECKOUT).expect("linux job");
        let macos_job = build_verification_task_job(&macos, CHECKOUT).expect("macos job");
        assert_eq!(linux_job.runs_on, "ubuntu-26.04");
        assert_eq!(macos_job.runs_on, "macos-15");

        let jobs = BTreeMap::from([(linux.job_id(), linux_job), (macos.job_id(), macos_job)]);
        let ids = validate_verification_jobs(&jobs, &[linux, macos], CHECKOUT)
            .expect("both platform jobs satisfy policy");
        assert_eq!(ids, ["task-linux-lint", "task-native-format"]);
    }

    #[test]
    fn all_declared_tasks_join_required_fan_in() {
        let linux = policy("linux-lint", VerificationRunner::LinuxX64);
        let macos = policy("native-format", VerificationRunner::MacosArm64);
        let mut jobs = BTreeMap::from([
            (
                linux.job_id(),
                build_verification_task_job(&linux, CHECKOUT).expect("linux job"),
            ),
            (
                macos.job_id(),
                build_verification_task_job(&macos, CHECKOUT).expect("macOS job"),
            ),
            (
                crate::render::FINAL_JOB_ID.to_owned(),
                Job {
                    display_name: "Required".to_owned(),
                    runs_on: "ubuntu-26.04".to_owned(),
                    timeout_minutes: JobTimeout::PLAN,
                    needs: vec!["plan".to_owned()],
                    condition: None,
                    permissions: None,
                    environment: None,
                    steps: Vec::new(),
                },
            ),
        ]);
        let ids = validate_verification_jobs(&jobs, &[linux, macos], CHECKOUT)
            .expect("all declared task jobs satisfy policy");

        extend_required_needs(&mut jobs, &ids).expect("required job exists");
        assert_eq!(
            jobs[crate::render::FINAL_JOB_ID].needs,
            vec![
                "plan".to_owned(),
                "task-linux-lint".to_owned(),
                "task-native-format".to_owned(),
            ]
        );
    }

    #[test]
    fn task_job_contract_rejects_conditions_dependencies_and_extra_steps() {
        let task = policy("native-format", VerificationRunner::MacosArm64);
        let mut job = build_verification_task_job(&task, CHECKOUT).expect("task job");
        job.condition = Some("always()".to_owned());
        let jobs = BTreeMap::from([(task.job_id(), job)]);
        let error = validate_verification_jobs(&jobs, &[task], CHECKOUT)
            .expect_err("conditional task cannot pass");
        assert!(error.to_string().contains("verification_job_contract"));
    }
}
