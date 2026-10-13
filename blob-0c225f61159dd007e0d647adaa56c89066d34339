//! Fixed rendering and policy for isolated native build-task jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    BuildTask, Job, JobTimeout, PermissionLevel, Permissions, Step, WORKFLOW_TASK_JOB_PREFIX,
};

use crate::verification_jobs::build_task_mise::{
    install_selected_tools_script, run_build_task_script, source_guard_script, validate_policy,
};
use crate::{MiseSetup, RenderError, mise_setup_step, shell_step, steps};

#[cfg(test)]
#[path = "build_task_jobs_tests.rs"]
mod tests;

/// Contract-fixed Xcode/SDK capability check step.
pub(crate) const VERIFY_BUILD_TASK_MACOS_NAME: &str = "Verify macOS build toolchain";
/// Contract-fixed MBX wrapper policy check step.
pub(crate) const VERIFY_BUILD_TASK_MBX_NAME: &str = "Verify locked MBX Rust route";
/// Contract-fixed declared Mise task step.
pub(crate) const RUN_BUILD_TASK_NAME: &str = "Run declared build task";
/// Contract-fixed selected-tool bootstrap step.
pub(crate) const INSTALL_BUILD_TASK_BOOTSTRAP_NAME: &str = "Install selected locked prebuilt tools";

/// Pinned Xcode developer directory required by the native task contract.
pub(crate) const BUILD_TASK_DEVELOPER_DIR: &str = "/Applications/Xcode_26.6.app/Contents/Developer";
/// Cargo registry tools must use prebuilt binaries; no Cargo compile fallback.
pub(crate) const CARGO_BINSTALL_ONLY_ENV: &str = "MISE_CARGO_BINSTALL_ONLY";

const MACOS_BUILD_TOOLCHAIN_CHECK: &str = concat!(
    "set -euo pipefail; ",
    "/usr/bin/xcodebuild -version | /usr/bin/grep -Fqx 'Xcode 26.6'; ",
    "/usr/bin/xcodebuild -version | /usr/bin/grep -Fqx 'Build version 17F113'; ",
    "/usr/bin/xcrun --sdk macosx --show-sdk-version | /usr/bin/grep -Fqx '26.5'",
);

/// One exact selected Mise tool resolved by the orchestrator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildTaskTool {
    /// Exact config key, preserving aliases and backend qualification.
    pub key: String,
    /// Exact version selected by the source config and lock.
    pub version: String,
    /// Exact lock-record backend.
    pub backend: String,
    /// Supported selected config option, currently only `os = ["macos"]`.
    pub os: Vec<String>,
    /// Resolved options bound to the source tool selector or Rust toolchain.
    pub config_options: BTreeMap<String, String>,
    /// Locked tool options; used for Rust components and targets.
    pub lock_options: BTreeMap<String, String>,
    /// Current-platform artifact for non-Rust tools.
    pub artifact: Option<BuildTaskArtifact>,
}

/// Exact current-platform prebuilt asset from the selected Mise lock row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildTaskArtifact {
    /// SHA-256 digest in Mise's `sha256:<hex>` form.
    pub checksum: String,
    /// Locked HTTPS download URL.
    pub url: String,
    /// Optional release API identity.
    pub url_api: Option<String>,
    /// Optional artifact signer identity.
    pub signer: Option<String>,
    /// Optional provenance policy label.
    pub provenance: Option<String>,
}

/// Per-task, orchestrator-resolved native runner and Mise binary pin.
#[derive(Debug, Clone)]
pub struct BuildTaskPolicy {
    /// Validated task declaration from `.velnor/config.toml`.
    pub task: BuildTask,
    /// Exact native runner label.
    pub runner_label: String,
    /// Mise setup action with the binary digest for this runner target.
    pub mise_setup: MiseSetup,
    /// SHA-256 digest of the project Mise config checked during planning.
    pub mise_config_sha256: String,
    /// SHA-256 digest of the project Mise lock checked during planning.
    pub mise_lock_sha256: String,
    /// SHA-256 digest of the idiomatic Rust toolchain checked during planning.
    pub rust_toolchain_sha256: String,
    /// SHA-256 of the task's declared Mise config, including its task bodies.
    pub source_mise_config_sha256: String,
    /// SHA-256 of a task-config-local Mise lock, when one exists.
    pub source_mise_lock_sha256: Option<String>,
    /// SHA-256 of a task-config-local Rust toolchain file, when one exists.
    pub source_rust_toolchain_sha256: Option<String>,
    /// Sorted selected tools with the current runner's locked artifacts.
    pub selected_tools: Vec<BuildTaskTool>,
}

impl BuildTaskPolicy {
    /// Generated GitHub job key for this declaration.
    #[must_use]
    pub fn job_id(&self) -> String {
        format!("{WORKFLOW_TASK_JOB_PREFIX}{}", self.task.id)
    }
}

/// Build the exact credential-scrubbed native job for one declaration.
/// # Errors
pub fn build_build_task_job(
    policy: &BuildTaskPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    validate_policy(policy)?;
    let timeout = JobTimeout::new(policy.task.timeout_minutes).map_err(RenderError::Contract)?;
    Ok(Job {
        display_name: format!("Build {}", policy.task.id),
        runs_on: policy.runner_label.clone(),
        timeout_minutes: timeout,
        needs: Vec::new(),
        condition: None,
        permissions: Some(build_task_permissions()),
        environment: None,
        steps: build_task_steps(policy, checkout_uses)?,
        check_runner: None,
    })
}

/// Validate every declared native job and return its sorted job IDs.
/// # Errors
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
    let developer_dir = BTreeMap::from([(
        "DEVELOPER_DIR".to_owned(),
        BUILD_TASK_DEVELOPER_DIR.to_owned(),
    )]);
    let resource_limits = BTreeMap::from([
        (
            "CARGO_BUILD_JOBS".to_owned(),
            policy.task.cargo_build_jobs.to_string(),
        ),
        (
            "NEXTEST_TEST_THREADS".to_owned(),
            policy.task.nextest_test_threads.to_string(),
        ),
        (
            "DEVELOPER_DIR".to_owned(),
            BUILD_TASK_DEVELOPER_DIR.to_owned(),
        ),
    ]);
    let script_env = [
        (
            "DEVELOPER_DIR".to_owned(),
            BUILD_TASK_DEVELOPER_DIR.to_owned(),
        ),
        (CARGO_BINSTALL_ONLY_ENV.to_owned(), "1".to_owned()),
    ];

    Ok(vec![
        steps::checkout_step(checkout_uses)?,
        shell_step(
            VERIFY_BUILD_TASK_MACOS_NAME,
            bash_script(MACOS_BUILD_TOOLCHAIN_CHECK),
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
    ])
}

/// Inline-shell argv so the script is single-quoted whole at render time.
///
/// Inner-shell variables must survive the outer shell; the script itself
/// starts with `set -euo pipefail`.
fn bash_script(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

fn build_task_permissions() -> Permissions {
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
