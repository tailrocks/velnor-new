//! Pinned tool catalog: exact mise tool selectors for every Velnor command.
//!
//! Versions are qualified pins (primary sources rechecked 2026-09-28): Rust
//! stable from `channel-rust-stable.toml`, the rest from GitHub releases or
//! the mise registry. Project `mise.toml` selectors never alter these pins.

use crate::error::MiseError;

/// Qualified mise runner release (tag `v2026.9.16`).
/// Source: `https://api.github.com/repos/jdx/mise/releases/latest`; checked 2026-09-28.
pub const MISE_VERSION: &str = "2026.9.16";
/// Qualified Rust stable toolchain.
/// Source: `https://static.rust-lang.org/dist/channel-rust-stable.toml`; checked 2026-09-28.
pub const RUST_VERSION: &str = "1.98.1";
/// Qualified `mr-boxington` tool (binary on PATH is `mbx`; tag `v1.19.0`).
/// Source: `https://api.github.com/repos/jdx/mr-boxington/releases/latest`; checked 2026-09-28.
pub const MR_BOXINGTON_VERSION: &str = "1.19.0";
/// Qualified GitHub CLI (tag `v2.101.0`).
/// Source: `https://api.github.com/repos/cli/cli/releases/latest`; checked 2026-09-28.
pub const GH_VERSION: &str = "2.101.0";
/// Qualified actionlint release (tag `v1.7.12`).
/// Source: `https://api.github.com/repos/rhysd/actionlint/releases/latest`; checked 2026-09-28.
pub const ACTIONLINT_VERSION: &str = "1.7.12";
/// Qualified shellcheck release (tag `v0.11.0`).
/// Source: `https://api.github.com/repos/koalaman/shellcheck/releases/latest`; checked 2026-09-28.
pub const SHELLCHECK_VERSION: &str = "0.11.0";
/// Qualified zizmor tool release (`zizmorcore/zizmor` tag `v1.30.1`, stable, published 2026-09-09).
/// Source: `https://api.github.com/repos/zizmorcore/zizmor/releases/latest`; checked 2026-09-28.
pub const ZIZMOR_VERSION: &str = "1.30.1";

/// Tools Velnor may select through mise, by registry name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinnedTool {
    /// Rust toolchain (`rust`, invoked as `cargo`/`rustc`).
    Rust,
    /// Mr Boxington compiler cache (`mr-boxington`, invoked as `mbx`).
    MrBoxington,
    /// GitHub CLI (`gh`).
    Gh,
    /// Workflow linter (`actionlint`).
    Actionlint,
    /// Shell linter (`shellcheck`).
    Shellcheck,
    /// Workflow security linter (`zizmor`).
    Zizmor,
}

impl PinnedTool {
    /// Every catalog tool in stable order.
    pub const ALL: [Self; 6] = [
        Self::Rust,
        Self::MrBoxington,
        Self::Gh,
        Self::Actionlint,
        Self::Shellcheck,
        Self::Zizmor,
    ];

    /// Mise registry name used in `<tool>@<version>` selectors.
    #[must_use]
    pub const fn tool_name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::MrBoxington => "mr-boxington",
            Self::Gh => "gh",
            Self::Actionlint => "actionlint",
            Self::Shellcheck => "shellcheck",
            Self::Zizmor => "zizmor",
        }
    }

    /// Resolve a registry name to its typed tool.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::UnknownTool`] for names outside the catalog.
    /// `mbx` and `mr_boxington` are rejected: the tool is `mr-boxington`.
    pub fn from_tool_name(name: &str) -> Result<Self, MiseError> {
        Self::ALL
            .iter()
            .find(|tool| tool.tool_name() == name)
            .copied()
            .ok_or_else(|| MiseError::UnknownTool {
                tool: name.to_owned(),
            })
    }
}

/// Exact version pins for every catalog tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCatalog {
    /// Pinned Rust toolchain version.
    rust: String,
    /// Pinned `mr-boxington` tool version.
    mr_boxington: String,
    /// Pinned GitHub CLI version.
    gh: String,
    /// Pinned actionlint version.
    actionlint: String,
    /// Pinned shellcheck version.
    shellcheck: String,
    /// Pinned zizmor tool version.
    zizmor: String,
}

impl ToolCatalog {
    /// Catalog holding the qualified pins.
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            rust: RUST_VERSION.to_owned(),
            mr_boxington: MR_BOXINGTON_VERSION.to_owned(),
            gh: GH_VERSION.to_owned(),
            actionlint: ACTIONLINT_VERSION.to_owned(),
            shellcheck: SHELLCHECK_VERSION.to_owned(),
            zizmor: ZIZMOR_VERSION.to_owned(),
        }
    }

    /// Catalog with explicit versions; every version must be an exact pin.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidToolVersion`] for the first version
    /// that is not exact `major.minor.patch`.
    pub fn new(
        rust: &str,
        mr_boxington: &str,
        gh: &str,
        actionlint: &str,
        shellcheck: &str,
        zizmor: &str,
    ) -> Result<Self, MiseError> {
        validate_exact_version(PinnedTool::Rust.tool_name(), rust)?;
        validate_exact_version(PinnedTool::MrBoxington.tool_name(), mr_boxington)?;
        validate_exact_version(PinnedTool::Gh.tool_name(), gh)?;
        validate_exact_version(PinnedTool::Actionlint.tool_name(), actionlint)?;
        validate_exact_version(PinnedTool::Shellcheck.tool_name(), shellcheck)?;
        validate_exact_version(PinnedTool::Zizmor.tool_name(), zizmor)?;
        Ok(Self {
            rust: rust.to_owned(),
            mr_boxington: mr_boxington.to_owned(),
            gh: gh.to_owned(),
            actionlint: actionlint.to_owned(),
            shellcheck: shellcheck.to_owned(),
            zizmor: zizmor.to_owned(),
        })
    }

    /// Exact pinned version for one tool.
    #[must_use]
    pub fn version(&self, tool: PinnedTool) -> &str {
        match tool {
            PinnedTool::Rust => &self.rust,
            PinnedTool::MrBoxington => &self.mr_boxington,
            PinnedTool::Gh => &self.gh,
            PinnedTool::Actionlint => &self.actionlint,
            PinnedTool::Shellcheck => &self.shellcheck,
            PinnedTool::Zizmor => &self.zizmor,
        }
    }

    /// Mise selector `<tool>@<exact>` for one tool.
    #[must_use]
    pub fn tool_spec(&self, tool: PinnedTool) -> String {
        format!("{}@{}", tool.tool_name(), self.version(tool))
    }

    /// Mise selectors for several tools, in the given order.
    #[must_use]
    pub fn tool_specs(&self, tools: &[PinnedTool]) -> Vec<String> {
        tools.iter().map(|tool| self.tool_spec(*tool)).collect()
    }
}

/// Reject loose selectors: only exact `major.minor.patch` pins qualify.
///
/// # Errors
///
/// Returns [`MiseError::InvalidToolVersion`] for empty strings, a leading
/// `v`, `latest`, two-part versions, or non-numeric components.
pub fn validate_exact_version(tool: &str, version: &str) -> Result<(), MiseError> {
    let exact = version.split('.').collect::<Vec<_>>();
    let [major, minor, patch] = exact.as_slice() else {
        return Err(invalid_version(tool, version));
    };
    for part in [major, minor, patch] {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid_version(tool, version));
        }
    }
    Ok(())
}

/// Build the shared rejection for a non-exact version string.
fn invalid_version(tool: &str, version: &str) -> MiseError {
    MiseError::InvalidToolVersion {
        tool: tool.to_owned(),
        version: version.to_owned(),
    }
}
