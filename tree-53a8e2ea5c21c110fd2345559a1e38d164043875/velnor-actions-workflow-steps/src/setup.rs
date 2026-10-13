//! Pinned Mise setup pins plus the legacy cache-off template.
//!
//! Pins arrive as typed [`MiseSetup`] from the orchestrator's compiled
//! catalog; the renderer never invents them. Strict insertion with the
//! qualified built-in cache lives in `cache_p08` (P08); this module keeps
//! the pin type, its validation, and the legacy `cache:false` template
//! for fixtures and upgrade inputs.

use velnor_actions_contract_workflow::{Step, StepRole};

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

/// Verified extracted binary digests for Mise 2026.9.18.
/// Official release SHASUMS256.txt matches each downloaded archive and its
/// corresponding raw binary checksum.
pub const MISE_BINARY_SHA256_LINUX_X64: &str =
    "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4";
/// Extracted Mise binary SHA-256 for macOS ARM64.
pub const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "484c135bd4329975d608d3f77e26c2ece5d2f5590f18ca71f44440294f8cfa6f";
/// Extracted Mise binary SHA-256 for macOS x86-64.
pub const MISE_BINARY_SHA256_MACOS_X64: &str =
    "02d8ba561847f996925e361262c0610a24f59fcd9e06ba9ed0b6022e19b317c3";

impl MiseSetup {
    /// Resolve a compiled binary pin for one job's target.
    /// # Errors
    /// Unsupported targets or versions have no qualified artifact.
    pub fn for_target(&self, target: &str) -> Result<Self, RenderError> {
        self.validate()?;
        if self.version != "2026.9.18" {
            return Err(RenderError::BadCommand(format!(
                "mise_setup_unqualified_version:{}",
                self.version
            )));
        }
        let sha256 = match target {
            "x86_64-unknown-linux-gnu" => MISE_BINARY_SHA256_LINUX_X64,
            "aarch64-apple-darwin" => MISE_BINARY_SHA256_MACOS_ARM64,
            "x86_64-apple-darwin" => MISE_BINARY_SHA256_MACOS_X64,
            _ => {
                return Err(RenderError::BadCommand(format!(
                    "mise_setup_unsupported_target:{target}"
                )));
            }
        };
        Ok(Self {
            sha256: sha256.to_owned(),
            ..self.clone()
        })
    }

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
    let mut step = steps::action_step(
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
    )?;
    step.role = Some(StepRole::MiseSetup);
    Ok(step)
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
