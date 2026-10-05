//! Pinned Mise setup pins and the isolated cache-off bootstrap template.
//!
//! Pins arrive as typed [`MiseSetup`] from the orchestrator's compiled
//! catalog; the renderer never invents them. Strict insertion with the
//! explicit canonical cache lives in `cache_p08` (P08); this module keeps
//! the pin type and one isolated `cache:false` bootstrap template.

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

/// `Setup Mise` step: exact pins, `cache:false`, isolated bootstrap home.
///
/// `install: false` keeps project tool files, tasks, and hooks from
/// running; `env: false` keeps Mise env out of subsequent steps.
/// The verified bootstrap has its own data root. Tool installs use a
/// separate fixed `MISE_DATA_DIR` in later typed command environments.
/// # Errors
pub fn mise_setup_step(setup: &MiseSetup) -> Result<Step, RenderError> {
    setup.validate()?;
    steps::action_step_with_env(
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
        std::collections::BTreeMap::from([
            (
                "MISE_DATA_DIR".to_owned(),
                crate::cache_steps::TOOLS_MISE_BOOTSTRAP_DATA_DIR.to_owned(),
            ),
            ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
            ("MISE_NO_ENV".to_owned(), "1".to_owned()),
            ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
            ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
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
