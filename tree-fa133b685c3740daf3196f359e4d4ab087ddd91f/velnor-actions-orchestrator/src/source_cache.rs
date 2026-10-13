//! P08 shared cache constructors: sources snapshot and Cargo-only fallback.
//!
//! The plan job saves the shared sources snapshot once (single writer);
//! crate jobs restore it read-only. Cargo-only repos (no MBX anywhere)
//! use pinned `rust-cache` (registry-only, shared key) instead; MBX and
//! mixed repos never do.

use std::collections::BTreeMap;

use velnor_actions_actionlint::{
    RUST_CACHE_ACTION_SHA, RUST_CACHE_ACTION_VERSION,
    actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION},
    rust_cache_inputs_schema, validate_action_inputs,
};
use velnor_actions_contract::{Step, StepKind};

use crate::OrchestratorError;

/// Owned Cargo home expression shared by writers and readers.
pub(crate) const SHARED_CARGO_HOME: &str = "${{ runner.temp }}/velnor/cargo";
/// Sources key prefix (shared: target + Rust + lock hash, never job id).
pub(crate) const SOURCES_KEY_PREFIX: &str = "velnor-v1-sources";
/// Cargo-only shared registry key prefix for `rust-cache`.
pub(crate) const RUST_CACHE_SHARED_PREFIX: &str = "velnor-cargo";

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
    sources_step(true, key, restore_keys)
}

/// Shared save step over the owned-home subset (writer only).
/// # Errors
///
/// Returns contract, actionlint, or render errors for rejected pins,
/// paths, or step shapes.
pub(crate) fn sources_save_step(key: &str) -> Result<Step, OrchestratorError> {
    sources_step(false, key, &[])
}

/// One sources restore/save step with the P08 display name.
fn sources_step(
    restore: bool,
    key: &str,
    restore_keys: &[String],
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
        let gate = velnor_actions_mise::cache_trust::authorize_trusted_save().map_err(wrap)?;
        step.condition = Some(gate.to_owned());
    }
    Ok(step)
}

/// Cargo-only registry step via pinned `rust-cache` (never with MBX).
///
/// Registry-only (`cache-targets: false`), shared key (no job id), explicit
/// `save-if` (writers true, readers false), never caches on failure.
/// # Errors
///
/// Returns contract or actionlint errors for bad keys, flags, or pins.
pub(crate) fn rust_cache_step(shared_key: &str, save_if: bool) -> Result<Step, OrchestratorError> {
    use velnor_actions_actionlint::PinnedActionRef;
    use velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;
    if !shared_key.starts_with(&format!("{RUST_CACHE_SHARED_PREFIX}-"))
        || shared_key.contains(' ')
        || shared_key.contains('\n')
        || shared_key.len() > MAX_CACHE_KEY_BYTES
    {
        return Err(bad_key(format!("bad_shared_key:{shared_key}")));
    }
    let uses = PinnedActionRef::new(
        "Swatinem/rust-cache",
        None,
        RUST_CACHE_ACTION_SHA,
        RUST_CACHE_ACTION_VERSION,
    )
    .map_err(OrchestratorError::from)?
    .uses_value();
    let with = BTreeMap::from([
        ("shared-key".to_owned(), shared_key.to_owned()),
        (
            "save-if".to_owned(),
            if save_if { "true" } else { "false" }.to_owned(),
        ),
        ("cache-targets".to_owned(), "false".to_owned()),
        ("cache-on-failure".to_owned(), "false".to_owned()),
        ("prefix-key".to_owned(), "velnor-v1-cargo".to_owned()),
        ("add-job-id-key".to_owned(), "false".to_owned()),
    ]);
    validate_action_inputs(&rust_cache_inputs_schema(), &with).map_err(OrchestratorError::from)?;
    Ok(Step {
        name: "Restore Cargo registry".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses,
            with,
            env: BTreeMap::new(),
        },
    })
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
