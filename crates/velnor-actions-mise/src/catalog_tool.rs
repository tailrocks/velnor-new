//! The closed set of tools Velnor may select through Mise.

use crate::error::MiseError;

/// Tools Velnor may select through mise, by registry name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PinnedTool {
    /// Rust toolchain (`rust`, invoked as `cargo`/`rustc`).
    Rust,
    /// Mr Boxington compiler cache (`mr-boxington`, invoked as `mbx`).
    MrBoxington,
    /// GitHub CLI (`gh`).
    Gh,
    /// Workflow linter (`actionlint`).
    Actionlint,
    /// Workflow, composite action and Dependabot linter (`jactionlint`).
    Jactionlint,
    /// Shell linter (`shellcheck`).
    Shellcheck,
    /// Workflow security linter (`zizmor`).
    Zizmor,
    /// Nextest test runner (`nextest`, invoked as `cargo nextest`).
    Nextest,
    /// `OpenTofu` engine (`opentofu`, invoked as `tofu`).
    Opentofu,
    /// Release coordinator (`release-plz`, const-pinned: no catalog slot).
    ReleasePlz,
    /// OCI registry client (`crane`) for image publication and promotion.
    Crane,
    /// JavaScript runtime and package manager (`bun`) for Bun workspace jobs.
    Bun,
}

impl PinnedTool {
    /// Every catalog tool in stable order.
    pub const ALL: [Self; 12] = [
        Self::Rust,
        Self::MrBoxington,
        Self::Gh,
        Self::Actionlint,
        Self::Jactionlint,
        Self::Shellcheck,
        Self::Zizmor,
        Self::Nextest,
        Self::Opentofu,
        Self::ReleasePlz,
        Self::Crane,
        Self::Bun,
    ];

    /// Mise registry name used in `<tool>@<version>` selectors.
    #[must_use]
    pub const fn tool_name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::MrBoxington => "mr-boxington",
            Self::Gh => "gh",
            Self::Actionlint => "actionlint",
            Self::Jactionlint => "jactionlint",
            Self::Shellcheck => "shellcheck",
            Self::Zizmor => "zizmor",
            Self::Nextest => "nextest",
            Self::Opentofu => "opentofu",
            Self::ReleasePlz => "release-plz",
            Self::Crane => "crane",
            Self::Bun => "bun",
        }
    }

    /// Resolve a registry name to its typed tool.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::UnknownTool`] for names outside the catalog.
    /// `mbx` and `mr_boxington` are rejected: the tool is `mr-boxington`.
    /// `cargo-nextest` is rejected: the tool is `nextest`.
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
