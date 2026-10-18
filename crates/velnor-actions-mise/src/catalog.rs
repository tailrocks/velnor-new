//! Pinned tool catalog: exact mise tool selectors for every Velnor command.
//!
//! Exact Velnor-owned pins; project `mise.toml` selectors never alter them.
//! Each source and check date is recorded beside its release constant.

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

/// Pinned identity records (ver §2) plus the trusted gate over them.
#[path = "catalog_identity.rs"]
mod identity;

/// Reviewed owned source staging identities, separate from distribution qualification.
#[path = "catalog_owned_source.rs"]
pub mod owned_source;

/// Exact-version validation plus qualification sources (split for size).
#[path = "catalog_versions.rs"]
mod versions;
pub use versions::{check_freshness_requirements, validate_exact_version};
use versions::{invalid_version, tool_source};

/// Adopted Mise runner release (tag `v2026.10.13`); local official-asset
/// digest and version qualification passed 2026-10-17. Hosted qualification
/// is required before release.
/// Source: `https://api.github.com/repos/jdx/mise/releases/tags/v2026.10.13`; checked 2026-10-17.
pub const MISE_VERSION: &str = "2026.10.13";
/// Qualified Rust stable toolchain; 1.99.0 is held pending policy review.
/// Source: `https://static.rust-lang.org/dist/channel-rust-stable.toml`; checked 2026-10-08.
pub const RUST_VERSION: &str = "1.98.1";
/// Qualified `mr-boxington` tool (binary on PATH is `mbx`; tag `v1.21.1`).
/// Source: `https://api.github.com/repos/jdx/mr-boxington/releases/latest`; checked 2026-10-03.
pub const MR_BOXINGTON_VERSION: &str = "1.21.1";
/// Qualified GitHub CLI (tag `v2.102.0`).
/// Source: `https://api.github.com/repos/cli/cli/releases/latest`; checked 2026-09-30.
pub const GH_VERSION: &str = "2.102.0";
/// Qualified actionlint release (tag `v1.7.12`).
/// Source: `https://api.github.com/repos/rhysd/actionlint/releases/latest`; checked 2026-09-28.
pub const ACTIONLINT_VERSION: &str = "1.7.12";
/// Qualified shellcheck release (tag `v0.11.0`).
/// Source: `https://api.github.com/repos/koalaman/shellcheck/releases/latest`; checked 2026-09-28.
pub const SHELLCHECK_VERSION: &str = "0.11.0";
/// Qualified zizmor tool release (`zizmorcore/zizmor` tag `v1.31.1`, stable).
/// Its official macOS ARM64 binary passed version and repository audit checks
/// 2026-10-17. Source: `https://api.github.com/repos/zizmorcore/zizmor/releases/tags/v1.31.1`.
pub const ZIZMOR_VERSION: &str = "1.31.1";
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
}

impl PinnedTool {
    /// Every catalog tool in stable order.
    pub const ALL: [Self; 9] = [
        Self::Rust,
        Self::MrBoxington,
        Self::Gh,
        Self::Actionlint,
        Self::Shellcheck,
        Self::Zizmor,
        Self::Nextest,
        Self::Opentofu,
        Self::ReleasePlz,
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
    /// Whether `RUSTUP_TOOLCHAIN` is exported in step environments.
    export_rustup_toolchain: bool,
    /// Whether the repository `mise.toml` rust record sets `mr_boxington = true`,
    /// so the pinned-tool preflight must also install [`PinnedTool::MrBoxington`]
    /// at the catalog pin (mise's exec-time wrapper dispatches cargo through it;
    /// with `MISE_AUTO_INSTALL=false` a missing install falls back to the
    /// action-owned `mbx` on PATH, which may be a different version).
    rust_mr_boxington: bool,
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
            nextest: NEXTEST_VERSION.to_owned(),
            opentofu: OPENTOFU_VERSION.to_owned(),
            export_rustup_toolchain: true,
            rust_mr_boxington: false,
        }
    }

    /// Catalog with explicit versions; every version must be an exact pin.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidToolVersion`] for the first version
    /// that is not exact `major.minor.patch`.
    #[expect(
        clippy::too_many_arguments,
        reason = "catalog carries eight exact pins at once"
    )]
    pub fn new(
        rust: &str,
        mr_boxington: &str,
        gh: &str,
        actionlint: &str,
        shellcheck: &str,
        zizmor: &str,
        nextest: &str,
        opentofu: &str,
    ) -> Result<Self, MiseError> {
        validate_exact_version(PinnedTool::Rust.tool_name(), rust)?;
        validate_exact_version(PinnedTool::MrBoxington.tool_name(), mr_boxington)?;
        validate_exact_version(PinnedTool::Gh.tool_name(), gh)?;
        validate_exact_version(PinnedTool::Actionlint.tool_name(), actionlint)?;
        validate_exact_version(PinnedTool::Shellcheck.tool_name(), shellcheck)?;
        validate_exact_version(PinnedTool::Zizmor.tool_name(), zizmor)?;
        validate_exact_version(PinnedTool::Nextest.tool_name(), nextest)?;
        validate_exact_version(PinnedTool::Opentofu.tool_name(), opentofu)?;
        Ok(Self {
            rust: rust.to_owned(),
            mr_boxington: mr_boxington.to_owned(),
            gh: gh.to_owned(),
            actionlint: actionlint.to_owned(),
            shellcheck: shellcheck.to_owned(),
            zizmor: zizmor.to_owned(),
            nextest: nextest.to_owned(),
            opentofu: opentofu.to_owned(),
            export_rustup_toolchain: true,
            rust_mr_boxington: false,
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
            PinnedTool::Nextest => &self.nextest,
            PinnedTool::Opentofu => &self.opentofu,
            PinnedTool::ReleasePlz => RELEASE_PLZ_VERSION,
        }
    }

    /// Mise selector for one tool: `<tool>@<exact>`, except Nextest,
    /// which needs its backend-qualified aqua-registry path.
    #[must_use]
    pub fn tool_spec(&self, tool: PinnedTool) -> String {
        match tool {
            PinnedTool::Nextest => {
                format!("{NEXTEST_TOOL_SPEC_PREFIX}@{}", self.version(tool))
            }
            _ => format!("{}@{}", tool.tool_name(), self.version(tool)),
        }
    }

    /// Exact `rustup` toolchain name (`<rust-exact>-<target-triple>`) that
    /// `mise install rust@<exact>` creates; never an ambient toolchain.
    #[must_use]
    pub fn rust_toolchain_name(&self) -> String {
        self.rust_toolchain_name_for_host(RUST_TARGET_TRIPLE)
    }

    /// Exact `rustup` toolchain name on one build host triple.
    ///
    /// Cross-compile legs address the host toolchain (where `mise install`
    /// ran), never the target triple being built for.
    #[must_use]
    pub fn rust_toolchain_name_for_host(&self, host: &str) -> String {
        format!("{}-{host}", self.version(PinnedTool::Rust))
    }

    /// Mise selectors for several tools, in the given order.
    #[must_use]
    pub fn tool_specs(&self, tools: &[PinnedTool]) -> Vec<String> {
        tools.iter().map(|tool| self.tool_spec(*tool)).collect()
    }

    /// Exact `RUSTUP_TOOLCHAIN` value: the pinned Rust version.
    #[must_use]
    pub fn rustup_toolchain(&self) -> String {
        self.version(PinnedTool::Rust).to_owned()
    }

    /// Configure whether step environments export `RUSTUP_TOOLCHAIN`.
    #[must_use]
    pub const fn with_export_rustup_toolchain(mut self, export: bool) -> Self {
        self.export_rustup_toolchain = export;
        self
    }

    /// Whether the repository rust record sets `mr_boxington = true`, so the
    /// pinned-tool preflight must install [`PinnedTool::MrBoxington`] too.
    #[must_use]
    pub const fn rust_mr_boxington(&self) -> bool {
        self.rust_mr_boxington
    }

    /// Configure whether the pinned-tool preflight must install the
    /// repository's `mr-boxington` pin alongside the drivers.
    #[must_use]
    pub const fn with_rust_mr_boxington(mut self, flag: bool) -> Self {
        self.rust_mr_boxington = flag;
        self
    }

    /// Whether step environments export `RUSTUP_TOOLCHAIN`.
    #[must_use]
    pub const fn exports_rustup_toolchain(&self) -> bool {
        self.export_rustup_toolchain
    }

    /// `RUSTUP_TOOLCHAIN` value when exported, or `None` when repository toolchain file governs.
    #[must_use]
    pub fn rustup_toolchain_env(&self) -> Option<&str> {
        if self.export_rustup_toolchain {
            Some(&self.rust)
        } else {
            None
        }
    }

    /// Fail unless the action-installed MBX equals the catalog pin (action SHA alone never proves it).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidToolVersion`] for a loose or mismatched version.
    pub fn reconcile_action_mbx(&self, reported: &str) -> Result<(), MiseError> {
        validate_exact_version(PinnedTool::MrBoxington.tool_name(), reported)?;
        (reported == self.version(PinnedTool::MrBoxington))
            .then_some(())
            .ok_or_else(|| invalid_version(PinnedTool::MrBoxington.tool_name(), reported))
    }
}
