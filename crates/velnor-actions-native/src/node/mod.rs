//! Exact package-manager vectors, frozen lock evidence and shared phase ordering.

use velnor_actions_contract::config::{PackageScript, WorkloadConfig, WorkloadKind};
use velnor_actions_contract::{ContractError, FileIndex};

/// Fixed package-manager commands for the selected typed scripts.
#[must_use]
pub fn phases(workload: &WorkloadConfig) -> Vec<(&'static str, Vec<String>)> {
    let (manager, install) = if workload.kind == WorkloadKind::NodeCi {
        ("npm", vec!["npm".into(), "ci".into()])
    } else {
        (
            "bun",
            vec!["bun".into(), "install".into(), "--frozen-lockfile".into()],
        )
    };
    let mut phases = vec![("install", install)];
    phases.extend(workload.package_scripts().iter().map(|script| {
        (
            script.id(),
            vec![manager.into(), "run".into(), script.id().into()],
        )
    }));
    phases
}

/// Validate the package manager's lock at the declared workspace root.
/// # Errors
/// Reports a missing committed lock for the selected package manager.
pub fn validate_evidence(
    workload: &WorkloadConfig,
    index: &FileIndex,
) -> Result<(), ContractError> {
    let root = workload.root.as_str();
    let prefix = if root == "." {
        String::new()
    } else {
        format!("{root}/")
    };
    let (locks, problem): (&[&str], &str) = match workload.kind {
        WorkloadKind::BunCi => (&["bun.lock", "bun.lockb"], "workload_bun_lock_missing"),
        WorkloadKind::NodeCi => (&["package-lock.json"], "workload_node_lock_missing"),
        _ => return Ok(()),
    };
    if !locks
        .iter()
        .any(|lock| index.contains(&format!("{prefix}{lock}")))
    {
        return Err(ContractError::identity(
            "package_lock",
            format!("{problem}:{root}"),
        ));
    }
    Ok(())
}

/// Shared package phase ordering used for obligation rendering.
#[must_use]
pub fn rank(kind: &str) -> Option<u32> {
    match kind {
        "install" => Some(0),
        "lint" => Some(PackageScript::Lint.rank()),
        "typecheck" => Some(PackageScript::Typecheck.rank()),
        "check" => Some(PackageScript::Check.rank()),
        "build" => Some(PackageScript::Build.rank()),
        "test" => Some(PackageScript::Test.rank()),
        _ => None,
    }
}

/// Human name for package-specific checks.
#[must_use]
pub fn step_name(kind: &str) -> Option<&'static str> {
    match kind {
        "lint" => Some("Package lint"),
        "typecheck" => Some("Package typecheck"),
        "check" => Some("Package checks"),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
