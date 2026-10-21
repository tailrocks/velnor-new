//! Pinned tool catalog: exact mise tool selectors for every Velnor command.
//!
//! Exact Velnor-owned pins for tools the project does not declare; a tool
//! the project's Mise config declares takes the project's version instead
//! (hard decision 5, [`project`]). Each source and check date is recorded
//! beside its release constant.

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
pub use versions::{
    check_freshness_requirements, validate_concrete_exact_version, validate_exact_version,
};

/// Pinned Rust channel-manifest digests (verifiable `core:rust` installs).
#[path = "catalog_rust_manifests.rs"]
mod rust_manifests;
pub use rust_manifests::{RUST_CHANNEL_MANIFESTS, rust_channel_manifest_sha256};

/// Pinned jactionlint release digests (verifiable `jactionlint` installs).
#[path = "catalog_jactionlint_digests.rs"]
mod jactionlint_digests;
pub use jactionlint_digests::{
    JACTIONLINT_DIGESTS, JactionlintDigest, jactionlint_archive_sha256,
    jactionlint_executable_sha256,
};

/// The typed catalog tool names (`PinnedTool`).
#[path = "catalog_tool.rs"]
mod tool;
pub use tool::PinnedTool;

/// Project-declared versions and their run-time references (hard decision 5).
#[path = "catalog_project.rs"]
pub mod project;
use versions::tool_source;

/// Selected Mise release (tag `v2026.10.17`); official asset bytes and the signed
/// checksums file were verified 2026-10-21. Runtime and hosted
/// qualification remain part of the release gates.
/// Source: `https://api.github.com/repos/jdx/mise/releases/tags/v2026.10.17`.
pub const MISE_VERSION: &str = "2026.10.17";
/// Selected Rust stable toolchain (2026-10-15 official stable manifest).
/// Source: `https://static.rust-lang.org/dist/channel-rust-stable.toml`; checked 2026-10-18.
pub const RUST_VERSION: &str = "1.99.1";
/// Selected `mr-boxington` tool (binary on PATH is `mbx`; tag `v1.26.0`).
/// Source: `https://api.github.com/repos/jdx/mr-boxington/releases/latest`; checked 2026-10-21.
pub const MR_BOXINGTON_VERSION: &str = "1.26.0";
/// Qualified GitHub CLI (tag `v2.102.1`).
/// Source: `https://api.github.com/repos/cli/cli/releases/latest`; checked 2026-10-21.
pub const GH_VERSION: &str = "2.102.1";
/// Qualified actionlint release (tag `v1.7.12`).
/// Source: `https://api.github.com/repos/rhysd/actionlint/releases/latest`; checked 2026-09-28.
pub const ACTIONLINT_VERSION: &str = "1.7.12";
/// Qualified jactionlint release (`jdx/jactionlint` tag `v2.3.0`, stable,
/// published 2026-10-21). Install: Mise registry name `jactionlint`
/// (backend `github:jdx/jactionlint`), which checks the release-API digest
/// and the GitHub artifact attestation. Provenance was verified at this bump
/// (2026-10-22), never at generate time: `cosign verify-blob-attestation`
/// on `packslip.sigstore.json` (`--type https://packslip.dev/release/v1`,
/// identity `https://github.com/jdx/jactionlint/.github/workflows/release.yaml@refs/tags/v2.3.0`,
/// issuer `https://token.actions.githubusercontent.com`) and
/// `gh attestation verify -R jdx/jactionlint --signer-workflow jdx/jactionlint/.github/workflows/release.yaml`
/// both passed for the linux amd64, linux arm64, darwin arm64 and darwin
/// amd64 archives.
/// Per-platform digests: [`JACTIONLINT_DIGESTS`].
/// Source: `https://api.github.com/repos/jdx/jactionlint/releases/latest`; checked 2026-10-22.
pub const JACTIONLINT_VERSION: &str = "2.3.0";
/// Qualified shellcheck release (tag `v0.11.0`).
/// Source: `https://api.github.com/repos/koalaman/shellcheck/releases/latest`; checked 2026-09-28.
pub const SHELLCHECK_VERSION: &str = "0.11.0";
/// Qualified zizmor tool release (`zizmorcore/zizmor` tag `v1.31.1`, stable).
/// Its official macOS ARM64 binary passed version and repository audit checks
/// 2026-10-17. Source: `https://api.github.com/repos/zizmorcore/zizmor/releases/tags/v1.31.1`.
pub const ZIZMOR_VERSION: &str = "1.31.1";
/// Qualified cargo-nextest release (`nextest-rs/nextest` tag
/// `cargo-nextest-0.9.149`, published 2026-10-20).
/// Source: `https://crates.io/api/v1/crates/cargo-nextest`; checked 2026-10-21.
pub const NEXTEST_VERSION: &str = "0.9.149";
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
/// Selected release-plz coordinator release (tag `release-plz-v0.3.173`).
/// Source: `https://crates.io/api/v1/crates/release-plz`; published 2026-10-19, checked 2026-10-20.
pub const RELEASE_PLZ_VERSION: &str = "0.3.173";
/// Qualified `crane` (`google/go-containerregistry` tag `v0.22.1`, published
/// 2026-09-04): pushes OCI layouts and tags digests in the image publication jobs.
/// Source: `https://api.github.com/repos/google/go-containerregistry/releases/latest`; checked 2026-10-20.
/// Install: Mise registry name `crane` through the aqua backend, which checks
/// the release `checksums.txt` before the binary is used.
pub const CRANE_VERSION: &str = "0.22.1";
/// Qualified Bun (`oven-sh/bun` tag `bun-v1.4.3`): the package manager and
/// script runner of Bun workspace jobs. A project declares its own exact
/// version in `mise.toml` (hard decision 4); this pin applies only to
/// callers that declare none.
/// Source: `https://api.github.com/repos/oven-sh/bun/releases/latest`; checked 2026-10-20.
/// Install: Mise core `bun@<exact>`.
pub const BUN_VERSION: &str = "1.4.3";
/// Fixed Cosign signer helper (release tag `v3.1.4`), not installed by Mise.
/// Source release assets and GitHub API SHA-256 digest were verified 2026-10-21.
pub const COSIGN_VERSION: &str = "3.1.4";
/// Exact bytes in the pinned Linux amd64 Cosign executable; bounds transport and reads.
pub const COSIGN_LINUX_AMD64_BYTES: usize = 145_688_404;
/// Official Linux `x86_64` Cosign asset digest from the v3.1.4 release API.
pub const COSIGN_LINUX_AMD64_SHA256: &str =
    "424e2c97d0b45fd7a6b608a87f090f35633f4c23ece5f092d48bba11aaf4bbfa";

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
    /// Pinned jactionlint version.
    jactionlint: String,
    /// Pinned shellcheck version.
    shellcheck: String,
    /// Pinned zizmor tool version.
    zizmor: String,
    /// Pinned Nextest runner version.
    nextest: String,
    /// Pinned `OpenTofu` engine version.
    opentofu: String,
    /// Pinned release-plz coordinator version.
    release_plz: String,
    /// Pinned `crane` registry client version.
    crane: String,
    /// Pinned Bun version.
    bun: String,
    /// Project-declared tools and the sources declaring them.
    project: std::collections::BTreeSet<PinnedTool>,
    /// Whether project versions render as run-time references.
    emission: bool,
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
            jactionlint: JACTIONLINT_VERSION.to_owned(),
            shellcheck: SHELLCHECK_VERSION.to_owned(),
            zizmor: ZIZMOR_VERSION.to_owned(),
            nextest: NEXTEST_VERSION.to_owned(),
            opentofu: OPENTOFU_VERSION.to_owned(),
            release_plz: RELEASE_PLZ_VERSION.to_owned(),
            crane: CRANE_VERSION.to_owned(),
            bun: BUN_VERSION.to_owned(),
            project: std::collections::BTreeSet::new(),
            emission: false,
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
        for (tool, version) in [
            (PinnedTool::Rust, rust),
            (PinnedTool::MrBoxington, mr_boxington),
            (PinnedTool::Gh, gh),
            (PinnedTool::Actionlint, actionlint),
            (PinnedTool::Shellcheck, shellcheck),
            (PinnedTool::Zizmor, zizmor),
            (PinnedTool::Nextest, nextest),
            (PinnedTool::Opentofu, opentofu),
        ] {
            validate_concrete_exact_version(tool.tool_name(), version)?;
        }
        Ok(Self {
            rust: rust.to_owned(),
            mr_boxington: mr_boxington.to_owned(),
            gh: gh.to_owned(),
            actionlint: actionlint.to_owned(),
            jactionlint: JACTIONLINT_VERSION.to_owned(),
            shellcheck: shellcheck.to_owned(),
            zizmor: zizmor.to_owned(),
            nextest: nextest.to_owned(),
            opentofu: opentofu.to_owned(),
            release_plz: RELEASE_PLZ_VERSION.to_owned(),
            crane: CRANE_VERSION.to_owned(),
            bun: BUN_VERSION.to_owned(),
            project: std::collections::BTreeSet::new(),
            emission: false,
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
            PinnedTool::Jactionlint => &self.jactionlint,
            PinnedTool::Shellcheck => &self.shellcheck,
            PinnedTool::Zizmor => &self.zizmor,
            PinnedTool::Nextest => &self.nextest,
            PinnedTool::Opentofu => &self.opentofu,
            PinnedTool::ReleasePlz => &self.release_plz,
            PinnedTool::Crane => &self.crane,
            PinnedTool::Bun => &self.bun,
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
}
