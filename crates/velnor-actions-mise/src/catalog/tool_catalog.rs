//! Core pinned tool catalog state and identity validation.

use super::rust_desktop::{DESKTOP_RUST_VERSION, RustCompilerRole};
use super::versions::{invalid_version, tool_source};
use super::{
    ACTIONLINT_VERSION, ALINT_VERSION, BOLTFFI_VERSION, BUN_VERSION, CARGO_AUDIT_VERSION,
    CARGO_DENY_VERSION, CARGO_SEMVER_CHECKS_VERSION, GH_VERSION, GRADLE_VERSION, JAVA_VERSION,
    JQ_VERSION, MR_BOXINGTON_VERSION, NEXTEST_VERSION, NODE_VERSION, OPENTOFU_SHA256_LINUX_AMD64,
    OPENTOFU_VERSION, PERIPHERY_VERSION, PLACEHOLDER_DIGEST, PYTHON_VERSION, PinnedTool,
    RELEASE_PLZ_VERSION, REUSE_VERSION, RUBY_VERSION, RUST_VERSION, SHELLCHECK_VERSION,
    SWIFT_VERSION, SWIFTLINT_VERSION, TOOL_PLATFORMS, UV_VERSION, XCODEGEN_VERSION, ZIZMOR_VERSION,
    validate_exact_version,
};
use crate::error::MiseError;
use velnor_actions_contract::ToolIdentity;

/// Exact version pins for every catalog tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCatalog {
    /// Closed execution role; numeric root compiler authority stays independent.
    pub(super) rust_role: RustCompilerRole,
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

impl ToolCatalog {
    /// Catalog holding the qualified pins.
    ///
    /// Versions are compile-time literals, so no fallible check runs
    /// here; trusted identity validation is the separate
    /// [`Self::validate_identities`] gate, which fails until freshness
    /// binds real artifact digests (P03-8b).
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            rust_role: RustCompilerRole::RootLinux,
            rust: RUST_VERSION.to_owned(),
            mr_boxington: MR_BOXINGTON_VERSION.to_owned(),
            gh: GH_VERSION.to_owned(),
            actionlint: ACTIONLINT_VERSION.to_owned(),
            shellcheck: SHELLCHECK_VERSION.to_owned(),
            zizmor: ZIZMOR_VERSION.to_owned(),
            nextest: NEXTEST_VERSION.to_owned(),
            opentofu: OPENTOFU_VERSION.to_owned(),
        }
    }

    /// Catalog with explicit versions; every version must be an exact pin.
    ///
    /// Only version exactness is enforced here: the catalog owns pins,
    /// not artifact trust. Trusted identity validation is the separate
    /// [`Self::validate_identities`] gate (P03-8b).
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
            rust_role: RustCompilerRole::RootLinux,
            rust: rust.to_owned(),
            mr_boxington: mr_boxington.to_owned(),
            gh: gh.to_owned(),
            actionlint: actionlint.to_owned(),
            shellcheck: shellcheck.to_owned(),
            zizmor: zizmor.to_owned(),
            nextest: nextest.to_owned(),
            opentofu: opentofu.to_owned(),
        })
    }

    /// Pinned identity record for one tool (ver §2).
    ///
    /// The digest is the qualified artifact SHA-256 once freshness
    /// binds it (`OpenTofu` is bound since T15); every other slot
    /// keeps the explicitly unqualified placeholder, so its record
    /// never validates as trusted and no trust decision may consume
    /// it (P03-8b).
    #[must_use]
    pub fn tool_identity(&self, tool: PinnedTool) -> ToolIdentity {
        ToolIdentity {
            name: tool.tool_name().to_owned(),
            version: self.version(tool).to_owned(),
            source: tool_source(tool, self.version(tool)),
            platforms: if matches!(
                tool,
                PinnedTool::RustDesktop | PinnedTool::CargoSemverChecks
            ) {
                vec!["macos-26".to_owned()]
            } else {
                TOOL_PLATFORMS.iter().map(ToString::to_string).collect()
            },
            digest: self.tool_digest(tool).to_owned(),
        }
    }

    /// Qualified artifact digest for one tool, or the placeholder.
    ///
    /// Only the qualified `OpenTofu` pin carries its digest (contract
    /// §4.3: the `linux_amd64` `.tar.gz` SHA-256, matching the x64-Linux
    /// runner fleet); any other version keeps the placeholder so an
    /// unqualified pin never validates as trusted.
    fn tool_digest(&self, tool: PinnedTool) -> &str {
        if tool == PinnedTool::Opentofu && self.version(tool) == OPENTOFU_VERSION {
            OPENTOFU_SHA256_LINUX_AMD64
        } else {
            PLACEHOLDER_DIGEST
        }
    }

    /// Trusted gate over every catalog identity record.
    ///
    /// Fails closed with `placeholder_digest` for every tool whose
    /// artifact SHA-256 freshness has not bound yet (`OpenTofu` is
    /// bound; the rest stay placeholder); it flips to `Ok` only once
    /// every slot binds, so callers can treat success as qualified
    /// trust (P03-8b).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::Contract`] for the first invalid identity.
    pub fn validate_identities(&self) -> Result<(), MiseError> {
        for tool in PinnedTool::ALL {
            self.tool_identity(tool)
                .validate("catalog")
                .map_err(|err| MiseError::Contract {
                    problem: err.to_string(),
                })?;
        }
        Ok(())
    }

    /// Exact pinned version for one tool.
    #[must_use]
    pub fn version(&self, tool: PinnedTool) -> &str {
        match tool {
            PinnedTool::Rust => &self.rust,
            PinnedTool::RustDesktop => DESKTOP_RUST_VERSION,
            PinnedTool::MrBoxington => &self.mr_boxington,
            PinnedTool::Gh => &self.gh,
            PinnedTool::Actionlint => &self.actionlint,
            PinnedTool::Shellcheck => &self.shellcheck,
            PinnedTool::Zizmor => &self.zizmor,
            PinnedTool::Nextest => &self.nextest,
            PinnedTool::Opentofu => &self.opentofu,
            PinnedTool::ReleasePlz => RELEASE_PLZ_VERSION,
            PinnedTool::Bun => BUN_VERSION,
            PinnedTool::Swift => SWIFT_VERSION,
            PinnedTool::Ruby => RUBY_VERSION,
            PinnedTool::Reuse => REUSE_VERSION,
            PinnedTool::Java => JAVA_VERSION,
            PinnedTool::Gradle => GRADLE_VERSION,
            PinnedTool::Python => PYTHON_VERSION,
            PinnedTool::Uv => UV_VERSION,
            PinnedTool::CargoAudit => CARGO_AUDIT_VERSION,
            PinnedTool::CargoDeny => CARGO_DENY_VERSION,
            PinnedTool::CargoSemverChecks => CARGO_SEMVER_CHECKS_VERSION,
            PinnedTool::Alint => ALINT_VERSION,
            PinnedTool::Node => NODE_VERSION,
            PinnedTool::Boltffi => BOLTFFI_VERSION,
            PinnedTool::Xcodegen => XCODEGEN_VERSION,
            PinnedTool::Jq => JQ_VERSION,
            PinnedTool::SwiftLint => SWIFTLINT_VERSION,
            PinnedTool::Periphery => PERIPHERY_VERSION,
        }
    }

    /// Exact `rustup` toolchain name (`<rust-exact>-<target-triple>`) that
    /// `mise install rust@<exact>` creates; never an ambient toolchain.
    #[must_use]
    pub fn rust_toolchain_name(&self) -> String {
        format!(
            "{}-{}",
            self.version(self.compiler_tool()),
            self.rust_host().target_triple()
        )
    }

    /// Exact `RUSTUP_TOOLCHAIN` value: the pinned Rust version.
    ///
    /// Rendered Cargo steps set this override so an inspected project
    /// toolchain file cannot select another compiler.
    #[must_use]
    pub fn rustup_toolchain(&self) -> String {
        self.version(self.compiler_tool()).to_owned()
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
