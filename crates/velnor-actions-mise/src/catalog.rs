//! Pinned tool catalog: exact mise tool selectors for every Velnor command.
//!
//! Exact Velnor-owned pins; project `mise.toml` selectors never alter them.
//! Each source and check date is recorded beside its release constant.

use velnor_actions_contract::ToolIdentity;

use crate::error::MiseError;

/// Bootstrap lock and version-policy IO plus verification.
///
/// Hosted here because the crate root is frozen: `mise::catalog::lock` is
/// the canonical path for lock parsing and catalog-equality checks.
#[path = "lock.rs"]
pub mod lock;

#[path = "lock_verify.rs"]
mod lock_verify;

/// Pinned release-plz coordinator argv (root frozen: `mise::catalog::release_plz`).
#[path = "release_plz.rs"]
pub mod release_plz;

/// MBX provisioning modes (root frozen: `mise::catalog::mbx`).
#[path = "catalog_mbx.rs"]
pub mod mbx;
pub use mbx::MbxProvisioning;

/// Measured official bootstrap assets for source building only.
#[path = "catalog_source_build_bootstrap.rs"]
pub mod source_build_bootstrap;

/// Reviewed owned source staging identities, separate from distribution qualification.
#[path = "catalog_owned_source.rs"]
pub mod owned_source;

/// Exact-version validation plus qualification sources (split for size).
#[path = "catalog_versions.rs"]
mod versions;
pub use versions::{
    PYTHON_BINARY_SHA256_LINUX_X64, PYTHON_PBS_SHA256, PYTHON_PBS_SIZE_BYTES, PYTHON_PBS_URL,
};
pub use versions::{check_freshness_requirements, validate_exact_version};
use versions::{invalid_version, python_tool_spec, tool_source};

/// Adopted Mise runner release (tag `v2026.10.9`, published 2026-10-13).
/// Source: `https://api.github.com/repos/jdx/mise/releases/latest`; checked 2026-10-13.
/// Hosted digest qualification for this exact release was not re-run.
pub const MISE_VERSION: &str = "2026.10.9";
/// Selected Rust stable toolchain (`rustc 1.99.0 (b940084d7 2026-09-28)`).
/// Source: `https://static.rust-lang.org/dist/channel-rust-stable.toml`; checked 2026-10-13.
/// Hosted qualification for this compiler pin was not re-run.
pub const RUST_VERSION: &str = "1.99.0";
/// Selected `mr-boxington` tool (binary on PATH is `mbx`; tag `v1.24.0`, published 2026-10-13).
/// Source: `https://api.github.com/repos/jdx/mr-boxington/releases/latest`; checked 2026-10-13.
/// Local binary reports `mbx 1.24.0`. Hosted qualification for this pin was not re-run.
pub const MR_BOXINGTON_VERSION: &str = "1.24.0";
/// Qualified GitHub CLI (tag `v2.102.0`).
/// Source: `https://api.github.com/repos/cli/cli/releases/latest`; checked 2026-09-30.
pub const GH_VERSION: &str = "2.102.0";
/// Qualified actionlint release (tag `v1.7.12`).
/// Source: `https://api.github.com/repos/rhysd/actionlint/releases/latest`; checked 2026-09-28.
pub const ACTIONLINT_VERSION: &str = "1.7.12";
/// Qualified shellcheck release (tag `v0.11.0`).
/// Source: `https://api.github.com/repos/koalaman/shellcheck/releases/latest`; checked 2026-09-28.
pub const SHELLCHECK_VERSION: &str = "0.11.0";
/// Qualified zizmor tool release (`zizmorcore/zizmor` tag `v1.30.1`, stable, published 2026-09-09).
/// Source: `https://api.github.com/repos/zizmorcore/zizmor/releases/latest`; checked 2026-09-28.
pub const ZIZMOR_VERSION: &str = "1.30.1";
/// Qualified cargo-nextest release (`nextest-rs/nextest` tag
/// `cargo-nextest-0.9.148`, published 2026-10-07).
/// Source: `https://crates.io/api/v1/crates/cargo-nextest`; checked 2026-10-08.
pub const NEXTEST_VERSION: &str = "0.9.148";
/// Qualified `OpenTofu` engine release (tag `v1.13.1`).
/// Source: `https://github.com/opentofu/opentofu/releases/tag/v1.13.1`; checked 2026-10-02.
/// Install: bare `opentofu@<exact>` via the aqua backend (isolated probe
/// passed 2026-10-02); the mise version index lags the release, so pin
/// exact, never float.
pub const OPENTOFU_VERSION: &str = "1.13.1";
/// sha256 of `tofu_1.13.1_linux_amd64.tar.gz` (`v1.13.1` `SHA256SUMS`,
/// release-API `digest`, and fetched bytes agree; verified 2026-10-02).
/// This is the catalog digest: the runner fleet is x64-Linux (§4.3).
pub const OPENTOFU_SHA256_LINUX_AMD64: &str =
    "378ada19d4bc70c43732004e8159be771b23b9a5afdf059e5f8a2b3fa2c70a69";
/// sha256 of `tofu_1.13.1_linux_arm64.tar.gz` (same `SHA256SUMS`, verified 2026-10-02).
pub const OPENTOFU_SHA256_LINUX_ARM64: &str =
    "9c1ef375aa1852db0b2888aa921b640c71f8140d4682aa4fec99378a64fa7dc3";
/// sha256 of `tofu_1.13.1_darwin_amd64.tar.gz` (same `SHA256SUMS`, verified 2026-10-02).
pub const OPENTOFU_SHA256_DARWIN_AMD64: &str =
    "a73720443ba38712d7d96dc1e857add02c15a790919c653ad07492e9952f8c27";
/// sha256 of `tofu_1.13.1_darwin_arm64.tar.gz` (same `SHA256SUMS`, verified 2026-10-02).
pub const OPENTOFU_SHA256_DARWIN_ARM64: &str =
    "be78f659f04ef06a9dbd9b3934d46af95d787a3aa38396d459dea395261816a9";
/// Selected release-plz coordinator release (tag `release-plz-v0.3.172`).
/// Source: `https://crates.io/api/v1/crates/release-plz`; published 2026-10-12, checked 2026-10-12.
pub const RELEASE_PLZ_VERSION: &str = "0.3.172";
/// REUSE lint package; isolated Python/uv workload probe passed 2026-10-02.
/// Source: `https://pypi.org/pypi/reuse/6.2.0/json`; checked 2026-10-02.
pub const REUSE_VERSION: &str = "6.2.0";
/// `CPython` upstream pin; Mise provisions Astral's standalone distribution.
/// Source: `https://www.python.org/downloads/`; checked 2026-10-02.
pub const PYTHON_VERSION: &str = "3.14.8";
/// uv runtime used by Mise's pipx backend for REUSE installation.
/// Source: `https://api.github.com/repos/astral-sh/uv/releases/latest`; checked 2026-10-02.
pub const UV_VERSION: &str = "0.12.22";

// Nextest needs its backend-qualified aqua-registry path: no `nextest`
// shorthand exists, `github:` tags carry a `cargo-nextest-` prefix, and
// `cargo:` compiles from source. Qualified 2026-09-29 via isolated probe.
const NEXTEST_TOOL_SPEC_PREFIX: &str = "aqua:nextest-rs/nextest/cargo-nextest";

/// Pinned Rust target triple: the single Linux target the runner fleet
/// maps to (`velnor_actions_contract::targets` stays the source of
/// truth; both runner labels resolve here).
pub const RUST_TARGET_TRIPLE: &str = "x86_64-unknown-linux-gnu";

/// Platforms every catalog tool supports (sorted, exact labels).
const TOOL_PLATFORMS: [&str; 2] = ["ubuntu-24.04", "ubuntu-26.04"];

/// Unqualified digest placeholder (ver §2): freshness replaces it per
/// upstream artifact sha256. Trusted validation rejects this value.
const PLACEHOLDER_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

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
    /// Nextest test runner (`nextest`, invoked as `cargo nextest`).
    Nextest,
    /// `OpenTofu` engine (`opentofu`, invoked as `tofu`).
    Opentofu,
    /// Release coordinator (`release-plz`, const-pinned: no catalog slot).
    ReleasePlz,
    /// REUSE license linter (`reuse`, installed through pipx).
    Reuse,
    /// `CPython` runtime (`python`).
    Python,
    /// Python package installer (`uv`).
    Uv,
}

impl PinnedTool {
    /// Every catalog tool in stable order.
    pub const ALL: [Self; 12] = [
        Self::Rust,
        Self::MrBoxington,
        Self::Gh,
        Self::Actionlint,
        Self::Shellcheck,
        Self::Zizmor,
        Self::Nextest,
        Self::Opentofu,
        Self::ReleasePlz,
        Self::Reuse,
        Self::Python,
        Self::Uv,
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
            Self::Nextest => "nextest",
            Self::Opentofu => "opentofu",
            Self::ReleasePlz => "release-plz",
            Self::Reuse => "reuse",
            Self::Python => "python",
            Self::Uv => "uv",
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
    /// Pinned Nextest runner version.
    nextest: String,
    /// Pinned `OpenTofu` engine version.
    opentofu: String,
}

include!("catalog.part2.rs");
