//! Pinned tool catalog: exact mise tool selectors for every Velnor command.
//!
//! Versions are qualified pins (primary sources rechecked 2026-09-28): Rust
//! stable from `channel-rust-stable.toml`, the rest from GitHub releases or
//! the mise registry. Project `mise.toml` selectors never alter these pins.

use velnor_actions_contract::{FreshnessRequirement, ToolIdentity, validate_freshness_class};

use crate::error::MiseError;

/// Bootstrap lock and version-policy IO plus verification.
///
/// Hosted here because the crate root is frozen: `mise::catalog::lock` is
/// the canonical path for lock parsing and catalog-equality checks.
#[path = "lock.rs"]
pub mod lock;

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
/// Qualified cargo-nextest release (`nextest-rs/nextest` tag
/// `cargo-nextest-0.9.146`, published 2026-09-21).
/// Source: `https://crates.io/api/v1/crates/cargo-nextest`; checked 2026-09-29.
pub const NEXTEST_VERSION: &str = "0.9.146";

// Backend-qualified mise selector prefix for Nextest. The mise registry
// has no `nextest` shorthand, so the catalog pins the aqua-registry path.
// Qualified 2026-09-29: `mise ls-remote` lists 0.9.146, `mise install
// aqua:nextest-rs/nextest/cargo-nextest@0.9.146` fetched the prebuilt
// universal-apple-darwin tarball in ~3s, and the isolated
// `mise --no-config --no-env --no-hooks exec rust@1.98.1 <spec> --
// cargo nextest --version` probe reported cargo-nextest 0.9.146.
// Rejected: `github:nextest-rs/nextest` tags carry a `cargo-nextest-`
// prefix (not an exact pin); `cargo:cargo-nextest` compiles from source.
const NEXTEST_TOOL_SPEC_PREFIX: &str = "aqua:nextest-rs/nextest/cargo-nextest";

/// Pinned Rust target triple: the single Linux target the runner fleet
/// maps to (`velnor_actions_contract::targets` stays the source of
/// truth; both runner labels resolve here).
pub const RUST_TARGET_TRIPLE: &str = "x86_64-unknown-linux-gnu";

/// Platforms every catalog tool supports (sorted, exact labels).
const TOOL_PLATFORMS: [&str; 2] = ["ubuntu-24.04", "ubuntu-26.04"];

/// Placeholder artifact digest pending live requalification (ver §2).
///
/// Shape-valid only: the freshness checker must replace this with each
/// upstream release-artifact sha256 before digest enforcement.
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
}

impl PinnedTool {
    /// Every catalog tool in stable order.
    pub const ALL: [Self; 7] = [
        Self::Rust,
        Self::MrBoxington,
        Self::Gh,
        Self::Actionlint,
        Self::Shellcheck,
        Self::Zizmor,
        Self::Nextest,
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
}

impl ToolCatalog {
    /// Catalog holding the qualified pins.
    #[must_use]
    pub fn pinned() -> Self {
        let catalog = Self {
            rust: RUST_VERSION.to_owned(),
            mr_boxington: MR_BOXINGTON_VERSION.to_owned(),
            gh: GH_VERSION.to_owned(),
            actionlint: ACTIONLINT_VERSION.to_owned(),
            shellcheck: SHELLCHECK_VERSION.to_owned(),
            zizmor: ZIZMOR_VERSION.to_owned(),
            nextest: NEXTEST_VERSION.to_owned(),
        };
        debug_assert!(catalog.validate_identities().is_ok());
        catalog
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
        nextest: &str,
    ) -> Result<Self, MiseError> {
        validate_exact_version(PinnedTool::Rust.tool_name(), rust)?;
        validate_exact_version(PinnedTool::MrBoxington.tool_name(), mr_boxington)?;
        validate_exact_version(PinnedTool::Gh.tool_name(), gh)?;
        validate_exact_version(PinnedTool::Actionlint.tool_name(), actionlint)?;
        validate_exact_version(PinnedTool::Shellcheck.tool_name(), shellcheck)?;
        validate_exact_version(PinnedTool::Zizmor.tool_name(), zizmor)?;
        validate_exact_version(PinnedTool::Nextest.tool_name(), nextest)?;
        let catalog = Self {
            rust: rust.to_owned(),
            mr_boxington: mr_boxington.to_owned(),
            gh: gh.to_owned(),
            actionlint: actionlint.to_owned(),
            shellcheck: shellcheck.to_owned(),
            zizmor: zizmor.to_owned(),
            nextest: nextest.to_owned(),
        };
        catalog.validate_identities()?;
        Ok(catalog)
    }

    /// Pinned identity record for one tool (ver §2).
    #[must_use]
    pub fn tool_identity(&self, tool: PinnedTool) -> ToolIdentity {
        ToolIdentity {
            name: tool.tool_name().to_owned(),
            version: self.version(tool).to_owned(),
            source: tool_source(tool, self.version(tool)),
            platforms: TOOL_PLATFORMS.iter().map(ToString::to_string).collect(),
            digest: PLACEHOLDER_DIGEST.to_owned(),
        }
    }

    /// Validate every catalog identity record.
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
            PinnedTool::MrBoxington => &self.mr_boxington,
            PinnedTool::Gh => &self.gh,
            PinnedTool::Actionlint => &self.actionlint,
            PinnedTool::Shellcheck => &self.shellcheck,
            PinnedTool::Zizmor => &self.zizmor,
            PinnedTool::Nextest => &self.nextest,
        }
    }

    /// Mise selector for one tool: `<tool>@<exact>`, except Nextest,
    /// which needs its backend-qualified aqua-registry path.
    ///
    /// Rust carries no inline options: bracketed tool options are silently
    /// ignored on config-less CLI specs, so components install through the
    /// fixed `PrepareRustComponents` step instead.
    #[must_use]
    pub fn tool_spec(&self, tool: PinnedTool) -> String {
        match tool {
            PinnedTool::Nextest => {
                format!("{NEXTEST_TOOL_SPEC_PREFIX}@{}", self.version(tool))
            }
            _ => format!("{}@{}", tool.tool_name(), self.version(tool)),
        }
    }

    /// Exact `rustup` toolchain name: `<rust-exact>-<target-triple>`.
    ///
    /// Addresses the toolchain `mise install rust@<exact>` creates, so the
    /// fixed component step cannot resolve an ambient toolchain instead.
    #[must_use]
    pub fn rust_toolchain_name(&self) -> String {
        format!("{}-{RUST_TARGET_TRIPLE}", self.version(PinnedTool::Rust))
    }

    /// Mise selectors for several tools, in the given order.
    #[must_use]
    pub fn tool_specs(&self, tools: &[PinnedTool]) -> Vec<String> {
        tools.iter().map(|tool| self.tool_spec(*tool)).collect()
    }

    /// Exact `RUSTUP_TOOLCHAIN` value: the pinned Rust version.
    ///
    /// Rendered Cargo steps set this override so an inspected project
    /// toolchain file cannot select another compiler.
    #[must_use]
    pub fn rustup_toolchain(&self) -> String {
        self.version(PinnedTool::Rust).to_owned()
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

/// How the effective MBX binary is provisioned (P07-10 contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MbxProvisioning {
    /// Mise installs the catalog pin; the action installs nothing.
    CatalogInstall,
    /// The action installs through its exact-version input (must equal the pin).
    ActionExactVersion,
    /// The action runs a preinstalled binary at this absolute path.
    PreinstalledTool {
        /// Absolute path to the preinstalled `mbx` binary.
        tool_path: String,
    },
}

impl MbxProvisioning {
    /// Preinstalled-tool mode for one validated absolute binary path.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] unless the path is absolute.
    pub fn preinstalled(tool_path: &str) -> Result<Self, MiseError> {
        if !tool_path.starts_with('/') || tool_path.contains('\0') {
            return Err(MiseError::InvalidStepInput {
                field: "tool_path".to_owned(),
                value: tool_path.to_owned(),
            });
        }
        Ok(Self::PreinstalledTool {
            tool_path: tool_path.to_owned(),
        })
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

/// Immutable qualification source for one tool at an exact version.
fn tool_source(tool: PinnedTool, version: &str) -> String {
    match tool {
        PinnedTool::Rust => "https://static.rust-lang.org/dist/channel-rust-stable.toml".to_owned(),
        PinnedTool::MrBoxington => {
            format!("https://github.com/jdx/mr-boxington/releases/tag/v{version}")
        }
        PinnedTool::Gh => format!("https://github.com/cli/cli/releases/tag/v{version}"),
        PinnedTool::Actionlint => {
            format!("https://github.com/rhysd/actionlint/releases/tag/v{version}")
        }
        PinnedTool::Shellcheck => {
            format!("https://github.com/koalaman/shellcheck/releases/tag/v{version}")
        }
        PinnedTool::Zizmor => {
            format!("https://github.com/zizmorcore/zizmor/releases/tag/v{version}")
        }
        PinnedTool::Nextest => {
            format!("https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-{version}")
        }
    }
}

/// Enforce per-class freshness requirements in Rust (ver §2).
///
/// # Errors
///
/// Returns [`MiseError::Contract`] for an unknown class or a violated bound.
pub fn check_freshness_requirements(
    requirements: &[FreshnessRequirement],
) -> Result<(), MiseError> {
    for requirement in requirements {
        validate_freshness_class(&requirement.class).map_err(|err| MiseError::Contract {
            problem: err.to_string(),
        })?;
        requirement
            .validate("freshness")
            .map_err(|err| MiseError::Contract {
                problem: err.to_string(),
            })?;
    }
    Ok(())
}
