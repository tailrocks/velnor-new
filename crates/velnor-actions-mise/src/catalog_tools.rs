//! Typed tools available through the pinned catalog.

use crate::error::MiseError;

/// Catalog identities Velnor may select through Mise, including closed roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinnedTool {
    /// Rust toolchain (`rust`, invoked as `cargo`/`rustc`).
    Rust,
    /// Closed native desktop compiler; root Rust pin remains independent.
    RustDesktop,
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
    /// Nextest test runner (`nextest`, invoked as `cargo nextest`).
    Nextest,
    /// `OpenTofu` engine (`opentofu`, invoked as `tofu`).
    Opentofu,
    /// Release coordinator (`release-plz`, const-pinned: no catalog slot).
    ReleasePlz,
    /// Bun JavaScript runtime and package manager.
    Bun,
    /// Swift compiler toolchain.
    Swift,
    /// Ruby interpreter and Bundler runtime.
    Ruby,
    /// REUSE license checker, installed through pipx.
    Reuse,
    /// Oracle `GraalVM` Java toolchain.
    Java,
    /// Gradle build tool.
    Gradle,
    /// Python runtime used by Python package tools.
    Python,
    /// uv Python package installer.
    Uv,
    /// Cargo vulnerability audit.
    CargoAudit,
    /// Cargo dependency policy checker.
    CargoDeny,
    /// Closed host-qualified Cargo API compatibility checker.
    CargoSemverChecks,
    /// Repository policy linter.
    Alint,
    /// Node.js runtime and bundled npm package manager.
    Node,
    /// Boltffi binding generator.
    Boltffi,
    /// Xcode project generator.
    Xcodegen,
    /// jq JSON processor.
    Jq,
    /// Swift style checker.
    SwiftLint,
    /// Swift unused code checker.
    Periphery,
}

impl PinnedTool {
    /// Every catalog tool in stable order.
    pub const ALL: [Self; 28] = [
        Self::Rust,
        Self::RustDesktop,
        Self::MrBoxington,
        Self::Gh,
        Self::Actionlint,
        Self::Shellcheck,
        Self::Zizmor,
        Self::Nextest,
        Self::Opentofu,
        Self::ReleasePlz,
        Self::Bun,
        Self::Swift,
        Self::Ruby,
        Self::Reuse,
        Self::Java,
        Self::Gradle,
        Self::Python,
        Self::Uv,
        Self::CargoAudit,
        Self::CargoDeny,
        Self::CargoSemverChecks,
        Self::Alint,
        Self::Node,
        Self::Boltffi,
        Self::Xcodegen,
        Self::Jq,
        Self::SwiftLint,
        Self::Periphery,
    ];

    /// Catalog identity name; [`crate::ToolCatalog::tool_spec`] owns the
    /// actual backend selector for closed compiler roles.
    #[must_use]
    pub const fn tool_name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::RustDesktop => "rust-desktop",
            Self::MrBoxington => "mr-boxington",
            Self::Gh => "gh",
            Self::Actionlint => "actionlint",
            Self::Shellcheck => "shellcheck",
            Self::Zizmor => "zizmor",
            Self::Nextest => "nextest",
            Self::Opentofu => "opentofu",
            Self::ReleasePlz => "release-plz",
            Self::Bun => "bun",
            Self::Swift => "swift",
            Self::Ruby => "ruby",
            Self::Reuse => "reuse",
            Self::Java => "java",
            Self::Gradle => "gradle",
            Self::Python => "python",
            Self::Uv => "uv",
            Self::CargoAudit => "cargo-audit",
            Self::CargoDeny => "cargo-deny",
            Self::CargoSemverChecks => "cargo-semver-checks",
            Self::Alint => "alint",
            Self::Node => "node",
            Self::Boltffi => "boltffi",
            Self::Xcodegen => "xcodegen",
            Self::Jq => "jq",
            Self::SwiftLint => "swiftlint",
            Self::Periphery => "periphery",
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
