//! Fixed runner-temp handoff for the unprivileged tap updater.

use super::{
    HomebrewTapPreparationProof, OrchestratorError, VerifiedPreviewRelease, internal,
    prepare_homebrew_tap,
};
use std::{env, fs, path::PathBuf};
use velnor_actions_contract::{canonical_json_bytes, config::HomebrewTapConfig, parse_strict_json};

/// Private CLI operation selecting native Homebrew tap preparation.
pub const PREPARE_HOMEBREW_TAP_OP: &str = "prepare-homebrew-tap-v1";

/// Execute tap preparation from the fixed, upstream-verified receipt handoff.
///
/// The retained signer artifact is reauthenticated here against the fixed
/// package-release plan and exact workflow identity. This boundary then checks
/// source configuration, captured tap main, task provenance, and formula bytes.
/// # Errors
/// Rejects missing or malformed request context and every preparation failure.
pub fn prepare_homebrew_tap_from_env() -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let workspace = absolute_env("GITHUB_WORKSPACE", "tap_workspace")?;
    let runner_temp = absolute_env("RUNNER_TEMP", "tap_runner_temp")?;
    let root = runner_temp.join("velnor");
    if !root.is_dir() || fs::symlink_metadata(&root).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(internal("tap_runner_temp_untrusted"));
    }
    let signed = crate::package_release_signer::verify_attested_signed_package_from_env()?;
    let receipt = signed.receipt().clone();
    let plan = signed.plan().clone();
    let receipt_digest = receipt
        .canonical_digest()
        .map_err(|_| internal("tap_prepare_receipt_digest"))?;
    let config = crate::config::load_config(&workspace)?;
    let tap = config
        .distribution
        .homebrew_tap
        .as_ref()
        .ok_or_else(|| internal("tap_prepare_policy_missing"))?;
    let static_policy = static_tap_policy()?;
    if canonical_json_bytes(tap).map_err(|_| internal("tap_policy_encode"))?
        != canonical_json_bytes(&static_policy).map_err(|_| internal("tap_policy_encode"))?
    {
        return Err(internal("tap_static_policy_mismatch"));
    }
    let preview = config
        .stacks
        .rust
        .as_ref()
        .map(|rust| &rust.binary_release.preview)
        .ok_or_else(|| internal("tap_prepare_preview_policy_missing"))?;
    let source_repository = crate::release_identity::origin_repository(&workspace)?;
    let release = VerifiedPreviewRelease {
        receipt_digest,
        receipt,
    };
    prepare_homebrew_tap(&source_repository, tap, preview, &plan, &release, &root)
}

pub(super) fn static_tap_policy() -> Result<HomebrewTapConfig, OrchestratorError> {
    const LIMIT: usize = 64 * 1024;
    let raw = env::var("VELNOR_TAP_POLICY").map_err(|_| internal("tap_static_policy_missing"))?;
    if raw.is_empty() || raw.len() > LIMIT {
        return Err(internal("tap_static_policy_invalid"));
    }
    let value = parse_strict_json(&raw).map_err(|_| internal("tap_static_policy_json"))?;
    let policy: HomebrewTapConfig =
        serde_json::from_value(value).map_err(|_| internal("tap_static_policy_schema"))?;
    policy
        .validate("VELNOR_TAP_POLICY")
        .map_err(|_| internal("tap_static_policy_invalid"))?;
    if !policy.enabled {
        return Err(internal("tap_static_policy_disabled"));
    }
    Ok(policy)
}

fn absolute_env(key: &str, problem: &str) -> Result<PathBuf, OrchestratorError> {
    let value = env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal(problem))?;
    if !value.is_absolute() {
        return Err(internal(problem));
    }
    Ok(value)
}
