//! Per-project override schema and action-input validation.
//!
//! Overrides are allowlisted by action key and approved `(sha, version)`
//! pairs; inputs validate against each action's allowlisted schema.

use crate::ActionlintError;
use crate::actions::{
    ALINT_ACTION, ALLOWED_ACTIONS, CHECKOUT_ACTION, PinnedActionRef, RUST_CACHE_ACTION,
    is_full_sha, is_version_tag, split_key,
};
use std::collections::{BTreeMap, BTreeSet};

/// Per-project pin override from `[actions.overrides]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPinOverride {
    /// Override key (`owner/repo[/path]`).
    pub action: String,
    /// Exact 40-char commit SHA.
    pub sha: String,
    /// Matching stable version (`vX.Y.Z`).
    pub version: String,
}

/// One approved `(sha, version)` pair for an allowlisted action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedPin {
    /// Exact 40-char commit SHA.
    pub sha: String,
    /// Matching stable version (`vX.Y.Z`).
    pub version: String,
}

/// Bundled approved-pin catalog: latest release plus compat pins.
///
/// The catalog data is supplied by the caller (compiled-in registry);
/// an empty catalog rejects every override (fail closed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovedPinCatalog {
    /// Approved pairs per allowlisted action key.
    pins: BTreeMap<String, Vec<ApprovedPin>>,
}

impl ApprovedPinCatalog {
    /// Empty catalog; rejects every override until populated.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert one approved pair for an overridable action.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError`] for unknown or policy-owned actions
    /// and for malformed pairs.
    pub fn insert(
        &mut self,
        action: &str,
        sha: &str,
        version: &str,
    ) -> Result<(), ActionlintError> {
        if !ALLOWED_ACTIONS.contains(&action) || is_policy_owned(action) {
            return Err(ActionlintError::OverrideRejected {
                action: action.to_owned(),
                problem: "action_not_overridable".to_owned(),
            });
        }
        if !is_full_sha(sha) || !is_version_tag(version) {
            return Err(ActionlintError::OverrideRejected {
                action: action.to_owned(),
                problem: "malformed_pair".to_owned(),
            });
        }
        self.pins
            .entry(action.to_owned())
            .or_default()
            .push(ApprovedPin {
                sha: sha.to_owned(),
                version: version.to_owned(),
            });
        Ok(())
    }

    /// Validate an override against the allowlist and catalog pairs.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError`] for unknown actions, policy-owned
    /// actions, malformed pairs, and pairs absent from the catalog.
    pub fn validate_override(
        &self,
        request: &ActionPinOverride,
    ) -> Result<PinnedActionRef, ActionlintError> {
        if !ALLOWED_ACTIONS.contains(&request.action.as_str()) {
            return Err(ActionlintError::UnknownAction {
                uses: request.action.clone(),
            });
        }
        if is_policy_owned(&request.action) {
            return Err(ActionlintError::OverrideRejected {
                action: request.action.clone(),
                problem: "action_not_overridable".to_owned(),
            });
        }
        if !is_full_sha(&request.sha) {
            return Err(ActionlintError::OverrideRejected {
                action: request.action.clone(),
                problem: "ref_must_be_full_sha".to_owned(),
            });
        }
        if !is_version_tag(&request.version) {
            return Err(ActionlintError::OverrideRejected {
                action: request.action.clone(),
                problem: format!("invalid_version:{}", request.version),
            });
        }
        let approved = self.pins.get(&request.action).cloned().unwrap_or_default();
        let matched = approved
            .iter()
            .any(|pin| pin.sha == request.sha && pin.version == request.version);
        if !matched {
            return Err(ActionlintError::OverrideRejected {
                action: request.action.clone(),
                problem: "unapproved_pair".to_owned(),
            });
        }
        let (repo, path) = split_key(&request.action);
        PinnedActionRef::new(&repo, path.as_deref(), &request.sha, &request.version)
    }
}

/// Allowlisted input schema for one action: only these inputs are valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInputSchema {
    /// Action key (`owner/repo[/path]`).
    pub action: String,
    /// Inputs the renderer must always supply.
    pub required: Vec<String>,
    /// Inputs the renderer may supply.
    pub optional: Vec<String>,
}

impl ActionInputSchema {
    /// Every input name the action accepts.
    #[must_use]
    pub fn allowed_inputs(&self) -> BTreeSet<String> {
        self.required
            .iter()
            .chain(self.optional.iter())
            .cloned()
            .collect()
    }
}

/// Canonical input schema for the no-credentials checkout step.
///
/// `persist-credentials` is required (always `false` in generated
/// jobs); `ref` and `fetch-depth` are the only other accepted inputs.
#[must_use]
pub fn checkout_inputs_schema() -> ActionInputSchema {
    ActionInputSchema {
        action: CHECKOUT_ACTION.to_owned(),
        required: vec!["persist-credentials".to_owned()],
        optional: vec!["ref".to_owned(), "fetch-depth".to_owned()],
    }
}

/// Canonical input schema for the Cargo-only `rust-cache` step (P08-7).
///
/// Required (always explicit, never defaulted): `shared-key` (one shared
/// registry identity, no job-id suffix), `save-if` (`true` only for the
/// trusted writer, `false` for readers), `cache-targets` (always `false`
/// in V1: registry only, targets stay per-lane ephemeral),
/// `cache-on-failure` (always `false`: producer-successful saves only).
/// Optional: `prefix-key`, `add-job-id-key` (always `false` when set).
/// Every other upstream input (`workspaces`, `cache-all-crates`, ...) is
/// rejected: V1 never sets per-job target caches over MBX-owned paths.
#[must_use]
pub fn rust_cache_inputs_schema() -> ActionInputSchema {
    ActionInputSchema {
        action: RUST_CACHE_ACTION.to_owned(),
        required: vec![
            "shared-key".to_owned(),
            "save-if".to_owned(),
            "cache-targets".to_owned(),
            "cache-on-failure".to_owned(),
        ],
        optional: vec!["prefix-key".to_owned(), "add-job-id-key".to_owned()],
    }
}

/// Validate action inputs against the action's allowlisted schema.
///
/// # Errors
///
/// Returns [`ActionlintError`] for unknown actions, unknown inputs,
/// missing required inputs, and empty or multi-line values.
pub fn validate_action_inputs(
    schema: &ActionInputSchema,
    inputs: &BTreeMap<String, String>,
) -> Result<(), ActionlintError> {
    if !ALLOWED_ACTIONS.contains(&schema.action.as_str()) {
        return Err(ActionlintError::UnknownAction {
            uses: schema.action.clone(),
        });
    }
    let allowed = schema.allowed_inputs();
    for name in inputs.keys() {
        if !allowed.contains(name) {
            return Err(ActionlintError::UnknownActionInput {
                action: schema.action.clone(),
                input: name.clone(),
            });
        }
    }
    for name in &schema.required {
        if !inputs.contains_key(name) {
            return Err(ActionlintError::MissingActionInput {
                action: schema.action.clone(),
                input: name.clone(),
            });
        }
    }
    for (name, value) in inputs {
        check_input_value(&schema.action, name, value)?;
    }
    Ok(())
}

/// Policy-owned actions are emittable but never consumer-overridable
/// (version-policy §2, GitHub Action defaults); mirrors the contract's
/// exclusion of alint from `OVERRIDABLE_ACTIONS`.
fn is_policy_owned(action: &str) -> bool {
    action == ALINT_ACTION
}

/// Reject empty and multi-line input values (YAML-injection safety).
fn check_input_value(action: &str, name: &str, value: &str) -> Result<(), ActionlintError> {
    if value.is_empty() {
        return Err(ActionlintError::InvalidActionInput {
            action: action.to_owned(),
            input: name.to_owned(),
            problem: "empty_value".to_owned(),
        });
    }
    if value.contains('\n') || value.contains('\r') {
        return Err(ActionlintError::InvalidActionInput {
            action: action.to_owned(),
            input: name.to_owned(),
            problem: "multiline_value".to_owned(),
        });
    }
    Ok(())
}
