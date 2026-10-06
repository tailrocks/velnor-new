//! Private repository-maintenance operations behind the existing CLI binary.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_freshness::Operation;

const ACTION_ENV: &str = "VELNOR_REPO_POLICY_ACTION";
const ROOT_ENV: &str = "VELNOR_REPO_POLICY_ROOT";
const UPSTREAM_ENV: &str = "VELNOR_REPO_POLICY_CHECK_UPSTREAM";
const ADVISORIES_ENV: &str = "VELNOR_REPO_POLICY_WITH_ADVISORIES";
const MESSAGE_PATH_ENV: &str = "VELNOR_REPO_POLICY_MESSAGE_PATH";
const LOCAL_IDENTITIES_ENV: &str = "VELNOR_REPO_POLICY_CHECK_LOCAL_IDENTITIES";

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
            | "trailer-policy"
    ) {
        return None;
    }
    let root = env::var_os(ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    if !root.is_dir() {
        return None;
    }
    if action == "trailer-policy" && !message_path()?.is_file() {
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
        Ok("trailer-policy") => Ok(Operation::TrailerPolicy {
            message_path: message_path().ok_or("missing or invalid message path")?,
            check_local_identities: optional_flag(LOCAL_IDENTITIES_ENV)?,
        }),
        _ => Err("invalid repository-policy action".to_owned()),
    }
}

fn message_path() -> Option<PathBuf> {
    let path = PathBuf::from(env::var_os(MESSAGE_PATH_ENV).filter(|value| !value.is_empty())?);
    (path.is_absolute() && path.is_file()).then_some(path)
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
