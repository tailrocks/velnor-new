//! Authenticated runner-temp admission for native tap preparation.

use super::{HomebrewTapPreparationProof, OrchestratorError, internal, prepare_homebrew_tap};
use std::{env, fs, path::PathBuf};
use velnor_actions_contract::{canonical_json_bytes, config::HomebrewTapConfig, parse_strict_json};

#[path = "tap_prepare_admission.rs"]
mod admission;
pub use admission::{ADMIT_HOMEBREW_TAP_OP, admit_homebrew_tap_from_env};

/// Private CLI operation selecting native Homebrew tap preparation.
pub const PREPARE_HOMEBREW_TAP_OP: &str = "prepare-homebrew-tap-v1";

/// Execute tap preparation from the fixed, upstream-verified receipt handoff.
///
/// A preceding source-free admission authenticates the retained signer artifact
/// against the plan and workflow identity, then exits. This token-free boundary checks
/// source configuration, captured tap main, canonical policy, and formula bytes.
/// # Errors
/// Rejects missing or malformed request context and every preparation failure.
pub fn prepare_homebrew_tap_from_env() -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    admission::reject_credentials()?;
    let deadline = admission::deadline()?;
    deadline
        .scope(|| prepare_admitted(deadline))
        .map_err(|error| internal(&error.to_string()))?
}

fn prepare_admitted(
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let workspace = absolute_env("GITHUB_WORKSPACE", "tap_workspace")?;
    let runner_temp = absolute_env("RUNNER_TEMP", "tap_runner_temp")?;
    let root = runner_temp.join("velnor");
    if !root.is_dir() || fs::symlink_metadata(&root).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(internal("tap_runner_temp_untrusted"));
    }
    let admitted = admission::load(&root, deadline)?;
    let receipt = admitted.receipt;
    let plan = admitted.plan;
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
    let source_repository = crate::release_identity::origin_repository(&workspace)?;
    let spec = static_formula_policy()?;
    match (config.distribution.homebrew_formula.as_ref(), spec.as_ref()) {
        (Some(source_spec), Some(spec))
            if canonical_json_bytes(source_spec)
                .map_err(|_| internal("tap_formula_spec_encode"))?
                == canonical_json_bytes(spec)
                    .map_err(|_| internal("tap_formula_spec_encode"))? => {}
        (None, None) => {}
        _ => return Err(internal("tap_static_formula_spec_mismatch")),
    }
    let sdk_artifact_digest = crate::generator::current_exe_sha256()
        .ok_or_else(|| internal("tap_sdk_artifact_digest_unavailable"))?;
    let release = super::VerifiedPackageRelease {
        receipt_digest,
        receipt,
    };
    prepare_homebrew_tap(crate::tap_prepare::TapPreparationInput {
        source_repository: &source_repository,
        tap,
        plan: &plan,
        release: &release,
        spec: spec.as_ref(),
        sdk_artifact_digest: &sdk_artifact_digest,
        temp: &root,
        deadline,
    })
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

pub(super) fn static_formula_policy()
-> Result<Option<velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec>, OrchestratorError> {
    const LIMIT: usize = 64 * 1024;
    let raw = match env::var("VELNOR_HOMEBREW_FORMULA_SPEC") {
        Ok(raw) => raw,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            return Err(internal("tap_static_formula_spec_invalid"));
        }
    };
    if raw.is_empty() || raw.len() > LIMIT {
        return Err(internal("tap_static_formula_spec_invalid"));
    }
    let value = parse_strict_json(&raw).map_err(|_| internal("tap_static_formula_spec_json"))?;
    let spec: velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec =
        serde_json::from_value(value).map_err(|_| internal("tap_static_formula_spec_schema"))?;
    let canonical = spec
        .canonical_bytes()
        .map_err(|_| internal("tap_static_formula_spec_invalid"))?;
    if canonical != raw.as_bytes() {
        return Err(internal("tap_static_formula_spec_not_canonical"));
    }
    Ok(Some(spec))
}

pub(super) fn absolute_env(key: &str, problem: &str) -> Result<PathBuf, OrchestratorError> {
    let value = env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal(problem))?;
    if !value.is_absolute() {
        return Err(internal(problem));
    }
    Ok(value)
}
