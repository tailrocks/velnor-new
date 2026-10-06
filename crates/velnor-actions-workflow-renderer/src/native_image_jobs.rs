//! Exact hosted native-platform image task jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, JobTimeout, NativeImageTask, PermissionLevel, Permissions, Step, WORKFLOW_TASK_JOB_PREFIX,
};

use crate::{RenderError, shell_step, steps};

#[cfg(test)]
#[path = "native_image_jobs_tests.rs"]
mod tests;

/// Per-task source binding resolved from the checked-out repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeImageTaskPolicy {
    /// Validated native-image task declaration.
    pub task: NativeImageTask,
    /// Exact hosted runner mapped from the closed platform enum.
    pub runner_label: String,
    /// SHA-256 of the tracked script bytes at planning time.
    pub source_sha256: String,
}

/// Verify the current checked-out script before invoking repository code.
pub(crate) const VERIFY_NATIVE_IMAGE_SOURCE_NAME: &str = "Verify native image task source";
/// Verify the runner and Docker daemon are native Linux ARM64.
pub(crate) const VERIFY_NATIVE_IMAGE_HOST_NAME: &str = "Verify native ARM64 Docker host";
/// Execute the source-bound native image script.
pub(crate) const RUN_NATIVE_IMAGE_TASK_NAME: &str = "Run native image validation";

const DOCKER_SOCKET: &str = "unix:///var/run/docker.sock";
/// Host capability probe with Docker client isolation owned by the script.
///
/// Inline-shell argv cannot carry the `env -u` prefix, so the script unsets
/// ambient Docker configuration and pins the socket before probing.
const HOST_CHECK_SCRIPT: &str = concat!(
    "set -euo pipefail; ",
    "unset DOCKER_CONFIG DOCKER_CONTEXT DOCKER_TLS_VERIFY DOCKER_CERT_PATH; ",
    "DOCKER_HOST=unix:///var/run/docker.sock; ",
    "export DOCKER_HOST; ",
    "/usr/bin/uname -s | /usr/bin/grep -Fqx 'Linux'; ",
    "/usr/bin/uname -m | /usr/bin/grep -Fqx 'aarch64'; ",
    "[[ \"${GITHUB_RUN_ID:-}\" =~ ^[0-9]+$ ]]; ",
    "[[ \"${GITHUB_RUN_ATTEMPT:-}\" =~ ^[0-9]+$ ]]; ",
    "/usr/bin/docker --host unix:///var/run/docker.sock info --format '{{.OSType}}/{{.Architecture}}' | /usr/bin/grep -Fqx -e 'linux/aarch64' -e 'linux/arm64'; ",
    "/usr/bin/docker --host unix:///var/run/docker.sock buildx version",
);

impl NativeImageTaskPolicy {
    /// Generated GitHub job key for this declaration.
    #[must_use]
    pub fn job_id(&self) -> String {
        format!("{WORKFLOW_TASK_JOB_PREFIX}{}", self.task.id)
    }
}

/// Build the exact native-host, least-privilege image validation job.
/// # Errors
pub fn build_native_image_job(
    policy: &NativeImageTaskPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    validate_policy(policy)?;
    let timeout = JobTimeout::new(policy.task.timeout_minutes).map_err(RenderError::Contract)?;
    Ok(Job {
        display_name: format!("Validate native image {}", policy.task.id),
        runs_on: policy.runner_label.clone(),
        timeout_minutes: timeout,
        needs: Vec::new(),
        condition: None,
        permissions: Some(native_image_permissions()),
        environment: None,
        steps: native_image_steps(policy, checkout_uses)?,
        check_runner: None,
    })
}

/// Reconstruct the closed native-image job and reject altered or split lanes.
/// # Errors
pub(crate) fn validate_native_image_jobs(
    jobs: &BTreeMap<String, Job>,
    policies: &[NativeImageTaskPolicy],
    checkout_uses: &str,
) -> Result<Vec<String>, RenderError> {
    let mut last_id = None;
    let mut seen = BTreeSet::new();
    let mut job_ids = Vec::with_capacity(policies.len());
    for policy in policies {
        validate_policy(policy)?;
        if last_id.is_some_and(|previous: &str| previous >= policy.task.id.as_str())
            || !seen.insert(policy.task.id.as_str())
        {
            return Err(RenderError::InvalidWorkflow(
                "native_image_tasks_not_sorted_unique".to_owned(),
            ));
        }
        let id = policy.job_id();
        let Some(actual) = jobs.get(&id) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "native_image_job_missing:{id}"
            )));
        };
        let expected = build_native_image_job(policy, checkout_uses)?;
        if !same_job_contract(actual, &expected) {
            return Err(RenderError::InvalidWorkflow(format!(
                "native_image_job_contract:{id}"
            )));
        }
        job_ids.push(id);
        last_id = Some(policy.task.id.as_str());
    }
    Ok(job_ids)
}

fn native_image_steps(
    policy: &NativeImageTaskPolicy,
    checkout_uses: &str,
) -> Result<Vec<Step>, RenderError> {
    Ok(vec![
        steps::checkout_step(checkout_uses)?,
        shell_step(
            VERIFY_NATIVE_IMAGE_SOURCE_NAME,
            bash_script(&source_guard_script(policy)),
            BTreeMap::new(),
        )?,
        shell_step(
            VERIFY_NATIVE_IMAGE_HOST_NAME,
            bash_script(HOST_CHECK_SCRIPT),
            BTreeMap::new(),
        )?,
        shell_step(
            RUN_NATIVE_IMAGE_TASK_NAME,
            image_script_argv(policy),
            BTreeMap::new(),
        )?,
    ])
}

fn source_guard_script(policy: &NativeImageTaskPolicy) -> String {
    let mut lines = vec![
        "set -euo pipefail".to_owned(),
        "cd -P -- \"$GITHUB_WORKSPACE\"".to_owned(),
    ];
    for (index, _) in policy.task.script.split('/').enumerate() {
        let component_count = index + 1;
        let prefix = policy
            .task
            .script
            .split('/')
            .take(component_count)
            .collect::<Vec<_>>()
            .join("/");
        lines.push(format!("test ! -L '{prefix}'"));
    }
    lines.extend([
        format!("test -f '{}'", policy.task.script),
        format!(
            "printf '%s  %s\\n' '{}' '{}' | /usr/bin/sha256sum --check --status",
            policy.source_sha256, policy.task.script
        ),
    ]);
    lines.join("; ")
}

fn image_script_argv(policy: &NativeImageTaskPolicy) -> Vec<String> {
    let mut argv = docker_environment();
    argv.extend([
        "/usr/bin/bash".to_owned(),
        "--".to_owned(),
        policy.task.script.clone(),
        policy.task.id.clone(),
        policy.task.platform.oci_platform().to_owned(),
    ]);
    argv
}

fn docker_environment() -> Vec<String> {
    vec![
        "/usr/bin/env".to_owned(),
        "-u".to_owned(),
        "DOCKER_CONFIG".to_owned(),
        "-u".to_owned(),
        "DOCKER_CONTEXT".to_owned(),
        "-u".to_owned(),
        "DOCKER_TLS_VERIFY".to_owned(),
        "-u".to_owned(),
        "DOCKER_CERT_PATH".to_owned(),
        format!("DOCKER_HOST={DOCKER_SOCKET}"),
    ]
}

/// Inline-shell argv so the script is single-quoted whole at render time.
///
/// Inner-shell variables must survive the outer shell; the script itself
/// starts with `set -euo pipefail`.
fn bash_script(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

fn validate_policy(policy: &NativeImageTaskPolicy) -> Result<(), RenderError> {
    policy
        .task
        .validate(".velnor/config.toml")
        .map_err(RenderError::Contract)?;
    if policy.runner_label != policy.task.platform.runs_on() {
        return Err(RenderError::InvalidWorkflow(format!(
            "native_image_runner_mismatch:{}",
            policy.task.id
        )));
    }
    if policy.source_sha256.len() != 64
        || !policy
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "native_image_source_digest:{}",
            policy.task.id
        )));
    }
    Ok(())
}

fn native_image_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
    }
}

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
