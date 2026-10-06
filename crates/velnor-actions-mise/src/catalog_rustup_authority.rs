//! Compiled manager expectations, never authentication of a restored tool tree.
//!
//! The bootstrap archive installs the identical executable as `bin/rustup`.
//! Its digest authenticates those bytes only. Settings, toolchains and actual
//! Cargo require the separate authenticated full-tool receipt before execution.

use super::rust_bootstrap::{RustHost, RustupBootstrap};
use super::rust_desktop::{DESKTOP_RUST_VERSION, RustCompilerRole};
use super::{RUST_VERSION, RustInstallOptions};
use velnor_actions_contract::ToolCacheDomain;

/// Immutable upstream source identity qualifying manager installation semantics.
/// This is not a reproducible archive-build proof or runtime receipt grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RustupManagerQualification {
    source_commit: &'static str,
    source_tree: &'static str,
}

impl RustupManagerQualification {
    /// Official annotated `1.29.1` tag's verified commit.
    #[must_use]
    pub const fn source_commit(self) -> &'static str {
        self.source_commit
    }

    /// Official commit's root source tree.
    #[must_use]
    pub const fn source_tree(self) -> &'static str {
        self.source_tree
    }

    /// Commit-bound installer source; installer copies its own executable bytes.
    #[must_use]
    pub fn source_url(self) -> String {
        format!(
            "https://github.com/rust-lang/rustup/blob/{}/src/cli/self_update.rs",
            self.source_commit
        )
    }
}

/// Sealed manager binding selected solely by a closed compiler role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RustupManagerAuthority {
    role: RustCompilerRole,
    domain: ToolCacheDomain,
    expected_path: &'static str,
    qualification: RustupManagerQualification,
}

impl RustupManagerAuthority {
    /// Select the full tool domain's compiled manager and compiler obligations.
    #[must_use]
    pub const fn for_role(role: RustCompilerRole) -> Self {
        Self {
            role,
            domain: ToolCacheDomain::Full,
            expected_path: "velnor/cargo/bin/rustup",
            qualification: RustupManagerQualification {
                source_commit: "d95a37b6ab92cc1e455d1576039333c97ca3e2c5",
                source_tree: "a346236d560c33eaaadced018b78581e3b45d245",
            },
        }
    }

    /// Closed role; repository settings cannot supply another host or pin.
    #[must_use]
    pub const fn role(self) -> RustCompilerRole {
        self.role
    }

    /// Rust manager state belongs solely to the full tools domain.
    #[must_use]
    pub const fn domain(self) -> ToolCacheDomain {
        self.domain
    }

    /// Actual runtime host required by the role, independent of cross targets.
    #[must_use]
    pub const fn host(self) -> RustHost {
        self.role.host()
    }

    /// Owned manager leaf relative to the admitted full domain's runner temp.
    /// Concrete runtime roots must come from the authenticated receipt owner.
    #[must_use]
    pub const fn expected_path(self) -> &'static str {
        self.expected_path
    }

    /// Exact manager version qualified by the official immutable archive.
    #[must_use]
    pub const fn version(self) -> &'static str {
        self.bootstrap().version()
    }

    /// One pin for both archive bytes and the copied installed executable.
    #[must_use]
    pub const fn sha256(self) -> &'static str {
        self.bootstrap().sha256()
    }

    /// Direct immutable official archive; never ambient PATH or latest.
    #[must_use]
    pub const fn archive_url(self) -> &'static str {
        self.bootstrap().url()
    }

    /// Source implementation qualifying the installer's manager byte copy.
    #[must_use]
    pub const fn qualification_descriptor(self) -> RustupManagerQualification {
        self.qualification
    }

    /// Exact fully resolved toolchain name admitted for this compiled role.
    #[must_use]
    pub fn selected_toolchain(self) -> String {
        format!(
            "{}-{}",
            self.compiler_version(),
            self.host().target_triple()
        )
    }

    /// Fixed minimal profile, components, targets and MBX policy for this role.
    #[must_use]
    pub fn options(self) -> RustInstallOptions {
        self.role.options()
    }

    /// Compiled compiler pin; no caller supplied numeric version is accepted.
    #[must_use]
    pub const fn compiler_version(self) -> &'static str {
        match self.role {
            RustCompilerRole::RootLinux | RustCompilerRole::ReleaseMac => RUST_VERSION,
            RustCompilerRole::DesktopMac | RustCompilerRole::DesktopSourceMac => {
                DESKTOP_RUST_VERSION
            }
        }
    }

    const fn bootstrap(self) -> RustupBootstrap {
        RustupBootstrap::for_host(self.host())
    }
}

#[cfg(test)]
#[path = "catalog_rustup_authority_tests.rs"]
mod tests;
