//! `[actions]` section: exact action-pin overrides.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use velnor_actions_contract::errors::ContractError;

/// Actions accepting per-project pin overrides (`[actions.overrides]` keys).
///
/// Intentional subset of `ALLOWED_ACTIONS` in
/// `crates/adapters/velnor-actions-actionlint/src/actions.rs`: the policy-owned
/// `asamarts/alint` pin is emittable but not consumer-overridable
/// (version-policy §2, GitHub Action defaults). The shape rules below
/// follow `overrides.rs` by convention (the contract cannot depend on
/// actionlint, which owns the approved `(sha, version)` catalog check).
pub const OVERRIDABLE_ACTIONS: [&str; 8] = [
    "jdx/mise-action",
    "actions/checkout",
    "actions/download-artifact",
    "actions/upload-artifact",
    "actions/cache/restore",
    "actions/cache/save",
    "jdx/mr-boxington-action",
    "Swatinem/rust-cache",
];

/// `[actions]` section: exact action-pin overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionsConfig {
    /// Pin overrides keyed by exact `owner/repo[/path]` action key.
    #[serde(default)]
    pub overrides: BTreeMap<String, ActionPinOverride>,
}

/// One `[actions.overrides]` value: exact SHA plus matching version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionPinOverride {
    /// Exact 40-char lowercase-hex commit SHA.
    pub sha: String,
    /// Matching stable version (`vX.Y.Z`).
    pub version: String,
}

impl ActionsConfig {
    /// Validate override keys (allowlist) and pin shapes.
    ///
    /// Catalog membership (approved `(sha, version)` pairs) is enforced by
    /// actionlint at generation time; the contract rejects unknown keys
    /// and malformed pins here.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        for (action, pin) in &self.overrides {
            let key_path = format!("actions.overrides.{action}");
            if !OVERRIDABLE_ACTIONS.contains(&action.as_str()) {
                return Err(ContractError::config(file, key_path, "unknown_action"));
            }
            if !is_full_sha(&pin.sha) {
                return Err(ContractError::config(
                    file,
                    key_path,
                    "ref_must_be_full_sha",
                ));
            }
            if !is_version_tag(&pin.version) {
                return Err(ContractError::config(
                    file,
                    key_path,
                    format!("invalid_version:{}", pin.version),
                ));
            }
        }
        Ok(())
    }
}

/// Full commit SHA: 40 lowercase hex characters.
fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Stable version tags: `vX.Y.Z` with numeric parts.
fn is_version_tag(value: &str) -> bool {
    let Some(number) = value.strip_prefix('v') else {
        return false;
    };
    let parts: Vec<&str> = number.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}
