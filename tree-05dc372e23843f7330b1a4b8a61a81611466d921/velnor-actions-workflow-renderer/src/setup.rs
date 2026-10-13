//! Pinned Mise setup pins plus the legacy cache-off template.
//!
//! Pins arrive as typed [`MiseSetup`] records in [`MiseSetupSet`] from
//! the orchestrator's compiled catalog; the renderer never invents them.
//! Strict insertion with the qualified built-in cache lives in
//! `cache_p08` (P08); this module keeps the pin types, validation, and
//! legacy `cache:false` template for fixtures and upgrade inputs.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use crate::{RenderError, steps};

/// Pinned Mise setup action name.
pub const MISE_ACTION_NAME: &str = "jdx/mise-action";
/// Contract-fixed display name of the setup step.
pub const SETUP_MISE_NAME: &str = "Setup Mise";

/// Typed Mise setup pins: action ref plus exact binary identity.
///
/// The orchestrator resolves `[actions.overrides]` and the compiled
/// catalog before constructing this; `sha256` is the digest of the
/// extracted `mise` binary for the single runner platform (the action
/// compares the input against the installed binary, not the archive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseSetup {
    /// Full-SHA `jdx/mise-action` ref.
    pub uses: String,
    /// Exact Mise release (`MISE_VERSION` catalog format, never `latest`).
    pub version: String,
    /// Lowercase hex SHA-256 of the installed `mise` binary.
    pub sha256: String,
}

/// Validated Mise setup pins for each runner target in one workflow.
///
/// The renderer receives this typed set from its orchestrator. It does
/// not choose versions or digests; it only selects the record matching
/// the target already resolved for a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseSetupSet {
    by_target: BTreeMap<String, MiseSetup>,
}

impl MiseSetupSet {
    /// Build a non-empty set of target-bound setup records.
    ///
    /// # Errors
    ///
    /// Returns a render error for an unsupported target, duplicate
    /// target, invalid setup pin, or an empty set.
    pub fn new(setups: impl IntoIterator<Item = (String, MiseSetup)>) -> Result<Self, RenderError> {
        let mut by_target = BTreeMap::new();
        for (target, setup) in setups {
            if !velnor_actions_contract::targets::is_supported_target(&target) {
                return Err(RenderError::BadCommand(format!(
                    "unsupported_mise_setup_target:{target}"
                )));
            }
            setup.validate()?;
            if by_target.insert(target.clone(), setup).is_some() {
                return Err(RenderError::BadCommand(format!(
                    "duplicate_mise_setup_target:{target}"
                )));
            }
        }
        if by_target.is_empty() {
            return Err(RenderError::BadCommand(
                "empty_mise_setup_targets".to_owned(),
            ));
        }
        Ok(Self { by_target })
    }

    /// Select the already-validated setup record for one target.
    ///
    /// # Errors
    ///
    /// Returns a render error when the workflow has no pin for `target`.
    pub fn for_target(&self, target: &str) -> Result<&MiseSetup, RenderError> {
        self.by_target.get(target).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("missing_mise_setup_target:{target}"))
        })
    }
}

impl MiseSetup {
    /// Validate every pin before any step is built from it.
    /// # Errors
    pub fn validate(&self) -> Result<(), RenderError> {
        steps::validate_uses(&self.uses)?;
        if !self.uses.starts_with(&format!("{MISE_ACTION_NAME}@")) {
            return Err(RenderError::BadActionRef(format!(
                "not_mise_action:{}",
                self.uses
            )));
        }
        if !is_catalog_version(&self.version) {
            return Err(RenderError::BadCommand(format!(
                "bad_mise_version:{}",
                self.version
            )));
        }
        if !velnor_actions_contract::ids::is_lower_hex_len(&self.sha256, 64) {
            return Err(RenderError::BadCommand("bad_mise_sha256".to_owned()));
        }
        Ok(())
    }
}

/// Legacy `Setup Mise` step: exact pins, `cache:false` (upgrade input).
///
/// `install: false` keeps project tool files, tasks, and hooks from
/// running; `env: false` keeps Mise env out of subsequent steps.
/// Retained for fixtures and as the upgrade input that strict rendering
/// replaces with the qualified built-in-cache shape (`cache:true` plus
/// an explicit `cache_key`, never the workspace-hashing default that
/// ELOOPs on symlink loops). The `with` map is exactly these six keys.
/// # Errors
pub fn mise_setup_step(setup: &MiseSetup) -> Result<Step, RenderError> {
    setup.validate()?;
    steps::action_step(
        SETUP_MISE_NAME,
        &setup.uses,
        std::collections::BTreeMap::from([
            ("version".to_owned(), setup.version.clone()),
            ("sha256".to_owned(), setup.sha256.clone()),
            ("install".to_owned(), "false".to_owned()),
            ("env".to_owned(), "false".to_owned()),
            ("cache".to_owned(), "false".to_owned()),
            ("cache_save".to_owned(), "false".to_owned()),
        ]),
    )
}

/// True for catalog version spellings (`2026.9.18`); never `latest`.
fn is_catalog_version(value: &str) -> bool {
    !value.is_empty()
        && value != "latest"
        && !value.contains("latest")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && value.contains('.')
        && !value.contains("${{")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(sha256: &str) -> MiseSetup {
        MiseSetup {
            uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
            version: "2026.10.2".to_owned(),
            sha256: sha256.to_owned(),
        }
    }

    #[test]
    fn setup_set_selects_only_the_requested_typed_record() {
        let linux_sha256 = "a".repeat(64);
        let macos_sha256 = "b".repeat(64);
        let linux = setup(&linux_sha256);
        let macos = setup(&macos_sha256);
        let setups = MiseSetupSet::new([
            ("x86_64-unknown-linux-gnu".to_owned(), linux.clone()),
            ("aarch64-apple-darwin".to_owned(), macos.clone()),
        ])
        .expect("valid target pins");
        assert_eq!(
            setups
                .for_target("x86_64-unknown-linux-gnu")
                .expect("Linux setup"),
            &linux
        );
        assert_eq!(
            setups
                .for_target("aarch64-apple-darwin")
                .expect("macOS setup"),
            &macos
        );
        assert!(
            setups
                .for_target("x86_64-apple-darwin")
                .is_err_and(|error| error.to_string().contains("missing_mise_setup_target"))
        );
    }

    #[test]
    fn setup_set_rejects_duplicate_unsupported_and_empty_targets() {
        let pin = setup(&"c".repeat(64));
        assert!(MiseSetupSet::new(Vec::new()).is_err());
        assert!(MiseSetupSet::new([("unknown-target".to_owned(), pin.clone())]).is_err());
        assert!(
            MiseSetupSet::new([
                ("x86_64-unknown-linux-gnu".to_owned(), pin.clone()),
                ("x86_64-unknown-linux-gnu".to_owned(), pin),
            ])
            .is_err()
        );
    }
}
