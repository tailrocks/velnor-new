// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    BuildTask, BuildTaskXcodeCapability, Job, JobTimeout, PermissionLevel, Permissions, Step,
    WORKFLOW_TASK_JOB_PREFIX,
};

use crate::verification_jobs::build_task_mise::{
    install_selected_tools_script, run_build_task_script, source_guard_script, validate_policy,
};
use crate::{MiseSetup, RenderError, mise_setup_step, shell_step, steps};

pub(crate) const VERIFY_BUILD_TASK_MACOS_NAME: &str = "Verify macOS build toolchain";
pub(crate) const VERIFY_BUILD_TASK_MBX_NAME: &str = "Verify locked MBX Rust route";
pub(crate) const RUN_BUILD_TASK_NAME: &str = "Run declared build task";
pub(crate) const INSTALL_BUILD_TASK_BOOTSTRAP_NAME: &str = "Install selected locked prebuilt tools";

pub(crate) const CARGO_BINSTALL_ONLY_ENV: &str = "MISE_CARGO_BINSTALL_ONLY";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildTaskTool {
    pub key: String,
    pub version: String,
    pub backend: String,
    pub os: Vec<String>,
    pub config_options: BTreeMap<String, String>,
    pub lock_options: BTreeMap<String, String>,
    pub artifact: Option<BuildTaskArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildTaskArtifact {
    pub checksum: String,
    pub url: String,
    pub url_api: Option<String>,
    pub signer: Option<String>,
    pub provenance: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BuildTaskPolicy {
    pub task: BuildTask,
    pub runner_label: String,
    pub mise_setup: MiseSetup,
    pub mise_config_sha256: String,
    pub mise_lock_sha256: String,
    pub rust_toolchain_sha256: String,
    pub source_mise_config_sha256: String,
    pub source_mise_lock_sha256: Option<String>,
    pub source_rust_toolchain_sha256: Option<String>,
    pub selected_tools: Vec<BuildTaskTool>,
}

impl BuildTaskPolicy {
    #[must_use]
    pub fn job_id(&self) -> String {
        format!("{WORKFLOW_TASK_JOB_PREFIX}{}", self.task.id)
    }
}

pub fn build_build_task_job(
    policy: &BuildTaskPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    validate_policy(policy)?;
    let timeout = JobTimeout::new(policy.task.timeout_minutes).map_err(RenderError::Contract)?;
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: format!("Build {}", policy.task.id),
        runs_on: policy.runner_label.clone(),
        timeout_minutes: timeout,
        needs: Vec::new(),
        condition: None,
        permissions: Some(build_task_permissions()),
        cache_mode: None,
        environment: None,
        steps: build_task_steps(policy, checkout_uses)?,
        check_runner: None,
    })
}

pub(crate) fn validate_build_task_jobs(
    jobs: &BTreeMap<String, Job>,
    policies: &[BuildTaskPolicy],
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
                "build_tasks_not_sorted_unique".to_owned(),
            ));
        }
        let id = policy.job_id();
        let Some(actual) = jobs.get(&id) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "build_task_job_missing:{id}"
            )));
        };
        let expected = build_build_task_job(policy, checkout_uses)?;
        if !same_job_contract(actual, &expected) {
            return Err(RenderError::InvalidWorkflow(format!(
                "build_task_job_contract:{id}"
            )));
        }
        job_ids.push(id);
        last_id = Some(policy.task.id.as_str());
    }
    Ok(job_ids)
}

fn build_task_steps(
    policy: &BuildTaskPolicy,
    checkout_uses: &str,
) -> Result<Vec<Step>, RenderError> {
    let xcode = policy.task.runner.xcode_capability();
    let developer_dir =
        BTreeMap::from([("DEVELOPER_DIR".to_owned(), xcode.developer_dir.to_owned())]);
    let resource_limits = BTreeMap::from([
        (
            "CARGO_BUILD_JOBS".to_owned(),
            policy.task.cargo_build_jobs.to_string(),
        ),
        (
            "NEXTEST_TEST_THREADS".to_owned(),
            policy.task.nextest_test_threads.to_string(),
        ),
        ("DEVELOPER_DIR".to_owned(), xcode.developer_dir.to_owned()),
    ]);
    let script_env = [
        ("DEVELOPER_DIR".to_owned(), xcode.developer_dir.to_owned()),
        (CARGO_BINSTALL_ONLY_ENV.to_owned(), "1".to_owned()),
    ];

    let mut job_steps = vec![
        steps::checkout_step(checkout_uses)?,
        shell_step(
            VERIFY_BUILD_TASK_MACOS_NAME,
            bash_script(&macos_build_toolchain_check(xcode)),
            developer_dir.clone(),
        )?,
        mise_setup_step(&policy.mise_setup)?,
        shell_step(
            INSTALL_BUILD_TASK_BOOTSTRAP_NAME,
            bash_script(&install_selected_tools_script(policy)?),
            script_env.iter().cloned().collect(),
        )?,
        shell_step(
            VERIFY_BUILD_TASK_MBX_NAME,
            bash_script(&source_guard_script(policy)?),
            script_env.iter().cloned().collect(),
        )?,
        shell_step(
            RUN_BUILD_TASK_NAME,
            bash_script(&run_build_task_script(policy)?),
            resource_limits,
        )?,
    ];
    for artifact in &policy.task.artifacts {
        let Some(path) = artifact.paths.first() else {
            return Err(RenderError::InvalidWorkflow(
                "build_task_artifact_path".to_owned(),
            ));
        };
        job_steps.push(steps::upload_build_task_artifact_step(
            &policy.task.id,
            &artifact.name,
            path,
            artifact.retention_days,
            artifact.when,
        )?);
    }
    Ok(job_steps)
}

fn macos_build_toolchain_check(capability: BuildTaskXcodeCapability) -> String {
    format!(
        concat!(
            "set -euo pipefail; ",
            "/usr/bin/xcodebuild -version | /usr/bin/grep -Fqx 'Xcode {}'; ",
            "/usr/bin/xcodebuild -version | /usr/bin/grep -Fqx 'Build version {}'; ",
            "/usr/bin/xcrun --sdk macosx --show-sdk-version | /usr/bin/grep -Fqx '{}'",
        ),
        capability.version, capability.build, capability.sdk_version
    )
}

fn bash_script(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

fn build_task_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
        packages: PermissionLevel::None,
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
