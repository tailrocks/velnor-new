//! Pinned lint-tool versions and minimal typed tool configs.
//!
//! Actionlint and `ShellCheck` versions are pinned constants (verified
//! 2026-09-28 per version policy). Zizmor has no spec-pinned version,
//! so its pin is explicit caller input validated for exactness. Types
//! only: Mise executes tools; this crate never spawns processes.

use crate::{ACTIONLINT_VERSION, ActionlintError};

/// Pinned `ShellCheck` release (tag `v0.11.0`; mirrors the mise catalog pin).
/// Source: `https://api.github.com/repos/koalaman/shellcheck/releases/latest`; checked 2026-09-28.
pub const SHELLCHECK_VERSION: &str = "0.11.0";

/// Pinned actionlint toolchain identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionlintToolchain {
    /// Exact pinned version (always [`ACTIONLINT_VERSION`]).
    version: String,
}

impl ActionlintToolchain {
    /// Toolchain at the pinned actionlint release.
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            version: ACTIONLINT_VERSION.to_owned(),
        }
    }

    /// Exact pinned version string.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Mise tool spec (`actionlint@<exact>`).
    #[must_use]
    pub fn mise_tool_spec(&self) -> String {
        format!("actionlint@{}", self.version)
    }

    /// Staged lint arguments for one pinned-binary invocation.
    ///
    /// Returns `["-no-color", "-oneline", "-config-file", config, ...workflows]`
    /// so the staged config and every staged workflow share a single
    /// invocation of the pinned binary. Pure data: the caller spawns via Mise.
    #[must_use]
    pub fn staged_lint_argv(config_path: &str, workflows: &[String]) -> Vec<String> {
        let mut argv = vec![
            "-no-color".to_owned(),
            "-oneline".to_owned(),
            "-config-file".to_owned(),
            config_path.to_owned(),
        ];
        argv.extend(workflows.iter().cloned());
        argv
    }
}

impl Default for ActionlintToolchain {
    fn default() -> Self {
        Self::pinned()
    }
}

/// Pinned `ShellCheck` toolchain identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellcheckToolchain {
    /// Exact pinned version (always [`SHELLCHECK_VERSION`]).
    version: String,
}

impl ShellcheckToolchain {
    /// Toolchain at the pinned `ShellCheck` release.
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            version: SHELLCHECK_VERSION.to_owned(),
        }
    }

    /// Exact pinned version string.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Mise tool spec (`shellcheck@<exact>`).
    #[must_use]
    pub fn mise_tool_spec(&self) -> String {
        format!("shellcheck@{}", self.version)
    }
}

impl Default for ShellcheckToolchain {
    fn default() -> Self {
        Self::pinned()
    }
}

/// Zizmor toolchain identity with an explicit exact pin.
///
/// No default: the spec pins no Zizmor version, so callers must supply
/// the reviewed exact release; anything else fails validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZizmorToolchain {
    /// Exact reviewed Zizmor release (`X.Y.Z`, no prefix).
    version: String,
}

impl ZizmorToolchain {
    /// Build a Zizmor toolchain from an explicit exact version.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError::InvalidToolVersion`] unless the version
    /// is exact `X.Y.Z` numeric with no prefix or prerelease.
    pub fn new(version: impl Into<String>) -> Result<Self, ActionlintError> {
        let version = version.into();
        if !is_exact_version(&version) {
            return Err(ActionlintError::InvalidToolVersion {
                tool: "zizmor",
                version,
            });
        }
        Ok(Self { version })
    }

    /// Exact pinned version string.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Mise tool spec (`zizmor@<exact>`).
    #[must_use]
    pub fn mise_tool_spec(&self) -> String {
        format!("zizmor@{}", self.version)
    }
}

/// Minimal typed config for the workflow-lint tool set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowLintTools {
    /// Pinned actionlint toolchain.
    pub actionlint: ActionlintToolchain,
    /// Pinned `ShellCheck` toolchain.
    pub shellcheck: ShellcheckToolchain,
    /// Explicitly pinned Zizmor toolchain.
    pub zizmor: ZizmorToolchain,
}

impl WorkflowLintTools {
    /// Build the lint-tool set from an explicit Zizmor version.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError::InvalidToolVersion`] for a non-exact
    /// Zizmor version.
    pub fn new(zizmor_version: impl Into<String>) -> Result<Self, ActionlintError> {
        Ok(Self {
            actionlint: ActionlintToolchain::pinned(),
            shellcheck: ShellcheckToolchain::pinned(),
            zizmor: ZizmorToolchain::new(zizmor_version)?,
        })
    }

    /// Mise tool specs in fixed order: actionlint, shellcheck, zizmor.
    #[must_use]
    pub fn mise_tool_specs(&self) -> Vec<String> {
        vec![
            self.actionlint.mise_tool_spec(),
            self.shellcheck.mise_tool_spec(),
            self.zizmor.mise_tool_spec(),
        ]
    }
}

/// Exact versions: `X.Y.Z` numeric, no `v` prefix, no prerelease.
fn is_exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}
