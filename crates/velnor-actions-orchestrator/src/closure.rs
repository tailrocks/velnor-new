//! Complete first-party input closures with explicit unknowns.
//! Declared via `#[path]` from `internal_plan.rs`; unknown inputs forbid reuse and coverage.
//!
//! Resolution dispatches per stack to the owning adapter; the closure
//! model itself lives in the contract crate.
use super::snapshot::normalize_checkout_path;
use std::path::Path;
#[cfg(test)]
pub(crate) use velnor_actions_contract::ClosureBuilder;
use velnor_actions_contract::{ContractError, ProposedTask, Stack, digest_b3};
pub(crate) use velnor_actions_contract::{Provenance, TaskInputClosure};

/// Resolve one proposed task's closure against the checkout at `root`.
///
/// Closed per-stack dispatch: each adapter resolves its own tasks.
/// Callers pass validated proposals, so unknown stacks and kind
/// spellings fail closed here instead of resolving silently.
///
/// # Errors
///
/// Returns [`ContractError`] for unregistered stacks and unknown
/// task-kind spellings.
pub(crate) fn resolve_closure_at_root(
    root: &Path,
    task: &ProposedTask,
    profile_nextest_config: Option<&str>,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
) -> Result<TaskInputClosure, ContractError> {
    match Stack::require_known(&task.stack_id)? {
        Stack::Rust => velnor_actions_rust::resolve_closure_at_root(
            root,
            task,
            profile_nextest_config,
            graph_digest,
            toolchain_id,
            platform_id,
        ),
    }
}
/// Provenance of one file: content digest, proven absence, or unknown.
fn probe_file(root: &Path, path: &str) -> Provenance {
    let Ok(normalized) = normalize_checkout_path(path) else {
        return Provenance::Unknown {
            reason: format!("bad_path:{path}"),
        };
    };
    match std::fs::read(root.join(&normalized)) {
        Ok(bytes) => Provenance::Known {
            digest: digest_b3(&bytes),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Provenance::AbsentProven {
            evidence: format!("not_found:{normalized}"),
        },
        Err(err) => Provenance::Unknown {
            reason: format!("unreadable:{normalized}:{err}"),
        },
    }
}
/// Decisive probes win; absence and guards fall through to the next probe.
macro_rules! triage {
    ($root:expr, $candidate:expr) => {
        match probe_file($root, $candidate) {
            known @ Provenance::Known { .. } => return known,
            unknown @ Provenance::Unknown { .. } => return unknown,
            Provenance::AbsentProven { .. } | Provenance::GuardedExternally { .. } => {}
        }
    };
}
/// Lockfile provenance, walking up from the manifest like Cargo does.
pub(crate) fn probe_lockfile(root: &Path, manifest: &str) -> Provenance {
    let mut dir = package_dir(manifest).to_owned();
    let mut probed = Vec::new();
    loop {
        let candidate = if dir.is_empty() {
            "Cargo.lock".to_owned()
        } else {
            format!("{dir}/Cargo.lock")
        };
        triage!(root, &candidate);
        probed.push(candidate);
        match dir.rsplit_once('/') {
            Some((parent, _)) => dir = parent.to_owned(),
            None if dir.is_empty() => break,
            None => dir.clear(),
        }
    }
    Provenance::AbsentProven {
        evidence: format!("not_found:{}", probed.join(",")),
    }
}
/// Nextest-config provenance: profile path plus the conventional path.
pub(crate) fn probe_nextest_config(root: &Path, profile_config: Option<&str>) -> Provenance {
    const CONVENTIONAL: &str = ".config/nextest.toml";
    if let Some(configured) = profile_config {
        triage!(root, configured);
        if configured == CONVENTIONAL {
            return Provenance::AbsentProven {
                evidence: format!("not_found:{CONVENTIONAL}"),
            };
        }
    }
    match probe_file(root, CONVENTIONAL) {
        known @ Provenance::Known { .. } => known,
        unknown @ Provenance::Unknown { .. } => unknown,
        Provenance::AbsentProven { evidence } => Provenance::AbsentProven {
            evidence: format!("profile:{profile_config:?}:{evidence}"),
        },
        guarded @ Provenance::GuardedExternally { .. } => guarded,
    }
}
fn package_dir(manifest: &str) -> &str {
    manifest.rsplit_once('/').map_or("", |(dir, _)| dir)
}
