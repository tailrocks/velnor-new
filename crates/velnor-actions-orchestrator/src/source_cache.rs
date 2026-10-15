//! P08 shared cache constructors for the exact Cargo sources subset.
//!
//! The plan job saves the shared sources snapshot once (single writer);
//! crate jobs restore it read-only. Cargo-only, MBX, and mixed repos use
//! this same archive so no action can overlap V2 tools-owned Cargo paths.

use velnor_actions_actionlint::actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION};
use velnor_actions_contract::{Step, StepRole};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;

/// Owned Cargo home expression shared by writers and readers.
pub(crate) const SHARED_CARGO_HOME: &str = "${{ runner.temp }}/velnor/cargo";
/// Sources key prefix (shared: target + Rust + lock hash, never job id).
pub(crate) const SOURCES_KEY_PREFIX: &str = "velnor-v1-sources";
/// Shared sources key: target + Rust + lock hash (no job id, no trust).
/// # Errors
///
/// Returns contract errors for unsupported targets, loose Rust versions,
/// unsafe roots, or overlong keys.
pub(crate) fn sources_cache_key(
    target: &str,
    rust_version: &str,
    roots: &[String],
) -> Result<String, OrchestratorError> {
    use velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(bad_key(format!("bad_target:{target}")));
    }
    velnor_actions_mise::validate_exact_version("rust", rust_version).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let mut locks = Vec::with_capacity(roots.len());
    for root in roots {
        crate::source_prep::validate_root(root)?;
        locks.push(if root.is_empty() {
            "Cargo.lock".to_owned()
        } else {
            format!("{root}/Cargo.lock")
        });
    }
    let quoted: Vec<String> = locks.iter().map(|lock| format!("'{lock}'")).collect();
    let key = format!(
        "{SOURCES_KEY_PREFIX}-{target}-{rust_version}-${{{{hashFiles({})}}}}",
        quoted.join(",")
    );
    if key.len() > MAX_CACHE_KEY_BYTES {
        return Err(bad_key("key_too_long".to_owned()));
    }
    Ok(key)
}

/// Same-compat restore prefix (snapshot omitted).
#[must_use]
pub(crate) fn sources_restore_prefix(key: &str) -> String {
    key.split_once("${{")
        .map_or_else(|| format!("{key}-"), |(head, _)| head.to_owned())
}

/// Shared restore step over the owned-home subset (readers and writer).
/// # Errors
///
/// Returns contract, actionlint, or render errors for rejected pins,
/// paths, or step shapes.
pub(crate) fn sources_restore_step(
    key: &str,
    restore_keys: &[String],
) -> Result<Step, OrchestratorError> {
    sources_step(
        true,
        key,
        restore_keys,
        velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    )
}

/// Shared save step over the owned-home subset (writer only).
/// # Errors
///
/// Returns contract, actionlint, or render errors for rejected pins,
/// paths, or step shapes.
pub(crate) fn sources_save_step(
    key: &str,
    policy: velnor_actions_contract::PullRequestCachePolicy,
) -> Result<Step, OrchestratorError> {
    sources_step(false, key, &[], policy)
}

/// One sources restore/save step with the P08 display name.
fn sources_step(
    restore: bool,
    key: &str,
    restore_keys: &[String],
    policy: velnor_actions_contract::PullRequestCachePolicy,
) -> Result<Step, OrchestratorError> {
    use velnor_actions_actionlint::PinnedActionRef;
    use velnor_actions_mise::cache_sources::{sources_cache_paths, validate_sources_subset};
    let wrap = |err: velnor_actions_mise::MiseError| OrchestratorError::Contract {
        problem: err.to_string(),
    };
    let paths = sources_cache_paths(SHARED_CARGO_HOME).map_err(wrap)?;
    validate_sources_subset(&paths, SHARED_CARGO_HOME).map_err(wrap)?;
    let op = if restore { "restore" } else { "save" };
    let uses = PinnedActionRef::new(
        "actions/cache",
        Some(op),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )
    .map_err(OrchestratorError::from)?
    .uses_value();
    let mut step = velnor_actions_workflow_renderer::steps::cache_action_step(
        restore,
        &uses,
        "sources",
        key,
        restore_keys,
        &paths,
    )
    .map_err(OrchestratorError::from)?;
    step.name = if restore {
        "Restore Cargo sources".to_owned()
    } else {
        "Save Cargo sources".to_owned()
    };
    if !restore {
        let gate = velnor_actions_contract::workflow::ir::cargo_sources_save_condition(policy);
        step.condition = Some(gate.to_owned());
    }
    step.role = Some(if restore {
        StepRole::CargoSourcesRestore
    } else {
        StepRole::CargoSourcesSave
    });
    Ok(step)
}

/// Lockless emits nothing, and tofu roles restore providers through
/// the separate provider-cache step (never here). The separate V2 tools
/// layer owns Cargo binaries and tool receipts. Compiler driver does not
/// change which shared-source payload readers restore.
pub(crate) fn restore_step_for_crate(
    label: &str,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    use_rust: bool,
) -> Result<Option<Step>, OrchestratorError> {
    if !use_rust || fetch_roots.is_empty() {
        return Ok(None);
    }
    let target = velnor_actions_contract::ReleaseTarget::for_runner_label(label)
        .map(velnor_actions_contract::ReleaseTarget::triple)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        })?;
    let rust = catalog.version(PinnedTool::Rust);
    let key = sources_cache_key(target, rust, fetch_roots)?;
    let prefix = sources_restore_prefix(&key);
    sources_restore_step(&key, &[prefix]).map(Some)
}

/// Shared-cache key rejection.
fn bad_key(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keys_reject_unsupported_execution_targets() {
        // Rust execution targets are charset-validated, so supported
        // configs can carry non-release triples; the cache path
        // hard-fails on them instead of keying a foreign toolchain.
        for target in [
            "aarch64-unknown-linux-gnu",
            "wasm32-unknown-unknown",
            "host",
        ] {
            let err = sources_cache_key(target, "1.98.1", &[]).expect_err("target");
            assert!(err.to_string().contains("bad_target"), "{target}: {err}");
        }
        for target in velnor_actions_contract::ReleaseTarget::ALL {
            let key = sources_cache_key(target.triple(), "1.98.1", &[]).expect("supported");
            assert!(key.starts_with(SOURCES_KEY_PREFIX), "{key}");
        }
        assert!(sources_cache_key("x86_64-unknown-linux-gnu", "1.98", &[]).is_err());
    }
}
