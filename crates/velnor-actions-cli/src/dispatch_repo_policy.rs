//! Private repository-maintenance operations behind the existing CLI binary.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_freshness::Operation;

const ACTION_ENV: &str = "VELNOR_REPO_POLICY_ACTION";
const ROOT_ENV: &str = "VELNOR_REPO_POLICY_ROOT";
const UPSTREAM_ENV: &str = "VELNOR_REPO_POLICY_CHECK_UPSTREAM";
const ADVISORIES_ENV: &str = "VELNOR_REPO_POLICY_WITH_ADVISORIES";

/// Accept only the private action allowlist and a repository root directory.
pub(crate) fn gate_root() -> Option<PathBuf> {
    let action = env::var(ACTION_ENV).ok()?;
    if !matches!(
        action.as_str(),
        "freshness"
            | "toolchain-specs"
            | "mise-version"
            | "workspace-members"
            | "library-members"
            | "artifact-executable"
    ) {
        return None;
    }
    let root = env::var_os(ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    if !root.is_dir() {
        return None;
    }
    Some(root)
}

/// Run the allowlisted repository-maintenance operation selected by environment.
pub(crate) fn run(root: &Path) -> ExitCode {
    let operation = match operation_from_environment() {
        Ok(operation) => operation,
        Err(error) => return fail_internal(&error),
    };
    let code = velnor_actions_freshness::run(root, &operation).clamp(0, 255);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn operation_from_environment() -> Result<Operation, String> {
    match env::var(ACTION_ENV).as_deref() {
        Ok("freshness") => Ok(Operation::Freshness {
            check_upstream: optional_flag(UPSTREAM_ENV)?,
            with_advisories: optional_flag(ADVISORIES_ENV)?,
        }),
        Ok("toolchain-specs") => Ok(Operation::ToolchainSpecs),
        Ok("mise-version") => Ok(Operation::MiseVersion),
        Ok("workspace-members") => Ok(Operation::WorkspaceMembers {
            libraries_only: false,
        }),
        Ok("library-members") => Ok(Operation::WorkspaceMembers {
            libraries_only: true,
        }),
        Ok("artifact-executable") => Ok(Operation::CargoArtifactExecutable {
            messages_path: required_path("VELNOR_REPO_POLICY_MESSAGES")?,
            manifest_path: required_path("VELNOR_REPO_POLICY_MANIFEST")?,
            target_name: required_text("VELNOR_REPO_POLICY_TARGET")?,
        }),
        _ => Err("invalid repository-policy action".to_owned()),
    }
}

fn required_text(name: &str) -> Result<String, String> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .ok_or_else(|| format!("{name} must contain nonempty text without control characters"))
}

fn required_path(name: &str) -> Result<PathBuf, String> {
    required_text(name).map(PathBuf::from)
}

fn optional_flag(name: &str) -> Result<bool, String> {
    match env::var(name) {
        Err(env::VarError::NotPresent) => Ok(false),
        Ok(value) if value == "0" => Ok(false),
        Ok(value) if value == "1" => Ok(true),
        Ok(_) | Err(env::VarError::NotUnicode(_)) => Err(format!("{name} must be absent, 0, or 1")),
    }
}

fn fail_internal(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: internal request failed: {problem}");
    ExitCode::from(1)
}
