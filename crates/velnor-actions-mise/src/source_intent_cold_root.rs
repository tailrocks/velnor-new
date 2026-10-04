//! Closed cold-install namespace for the `SourceIntent` SDK purpose.
//!
//! Pure generation data. No cache domain, restored payload or runtime grant
//! participates. Only the canonical fresh installer may mint runtime results.

use crate::catalog::rust_desktop::RustCompilerRole;
use crate::catalog::{RUST_VERSION, RustInstallOptions, rust_bootstrap::RustHost};

/// Fixed namespace kept outside every cached payload and planning tool root.
pub const SOURCE_INTENT_COLD_ROOT_EXPR: &str = "${{ runner.temp }}/velnor-control/source-intent";
/// Exact source-owned binding checked by the fresh installer runtime.
pub const SOURCE_INTENT_COLD_ROOT_ENV: &str = "VELNOR_SOURCE_INTENT_COLD_ROOT";

/// Owner-selected leaves; callers cannot provide paths or environment names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceIntentColdLeaf {
    /// Qualified Mise executable and isolated install data.
    Mise,
    /// Fresh Cargo proxies, without imported dependency/source state.
    Cargo,
    /// Fresh pinned manager metadata and compiler toolchain.
    Rustup,
    /// Fresh digest-verified Rustup installer scratch.
    RustupBootstrap,
    /// Empty exclusive Mise user configuration.
    MiseConfig,
    /// Empty exclusive Mise system configuration.
    MiseSystemConfig,
    /// Exclusive verified executable search directory.
    TrustedBin,
}

impl SourceIntentColdLeaf {
    /// All leaves owned and cleared by the canonical fresh installer.
    pub const ALL: [Self; 7] = [
        Self::Mise,
        Self::Cargo,
        Self::Rustup,
        Self::RustupBootstrap,
        Self::MiseConfig,
        Self::MiseSystemConfig,
        Self::TrustedBin,
    ];

    /// Fixed namespace-relative spelling, never caller controlled.
    #[must_use]
    pub const fn relative(self) -> &'static str {
        match self {
            Self::Mise => "mise",
            Self::Cargo => "cargo",
            Self::Rustup => "rustup-home",
            Self::RustupBootstrap => "rustup-bootstrap",
            Self::MiseConfig => "mise-config",
            Self::MiseSystemConfig => "mise-system-config",
            Self::TrustedBin => "trusted-bin",
        }
    }
}

/// Immutable, nonserializable `RootLinux` installation intent; no runtime power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceIntentColdRoot {
    private: (),
}

impl SourceIntentColdRoot {
    /// Select the sole approved fresh-install purpose without caller options.
    #[must_use]
    pub const fn root_linux() -> Self {
        Self { private: () }
    }

    /// Compiled root environment, bound into the canonical installer source.
    #[must_use]
    pub const fn namespace_environment(self) -> (&'static str, &'static str) {
        (SOURCE_INTENT_COLD_ROOT_ENV, SOURCE_INTENT_COLD_ROOT_EXPR)
    }

    /// Required concrete runner key; runtime capture owns path resolution.
    #[must_use]
    pub const fn runner_temp_environment(self) -> &'static str {
        "RUNNER_TEMP"
    }

    /// Fixed compiled location relative to runner temp, for source generation.
    /// Runtime callers cannot override this spelling with a path argument.
    #[must_use]
    pub const fn relative_to_runner_temp(self) -> &'static str {
        "velnor-control/source-intent"
    }

    /// Fixed owned leaves; runtime result admission follows fresh installation.
    #[must_use]
    pub const fn leaves(self) -> &'static [SourceIntentColdLeaf] {
        &SourceIntentColdLeaf::ALL
    }

    /// Exact root compiler pin, shared with its existing catalog authority.
    #[must_use]
    pub const fn compiler_version(self) -> &'static str {
        RUST_VERSION
    }

    /// Sole approved host; release and desktop roles cannot select this intent.
    #[must_use]
    pub const fn host(self) -> RustHost {
        RustCompilerRole::RootLinux.host()
    }

    /// Fixed `RootLinux` profile/components/targets, with no MBX wrapper.
    #[must_use]
    pub fn options(self) -> RustInstallOptions {
        RustCompilerRole::RootLinux.options()
    }

    /// Expression form for one closed leaf; no raw root or suffix accepted.
    #[must_use]
    pub fn leaf_expression(self, leaf: SourceIntentColdLeaf) -> String {
        format!("{SOURCE_INTENT_COLD_ROOT_EXPR}/{}", leaf.relative())
    }
}

#[cfg(test)]
#[path = "source_intent_cold_root_tests.rs"]
mod tests;
