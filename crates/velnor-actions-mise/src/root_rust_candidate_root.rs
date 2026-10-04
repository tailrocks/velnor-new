//! Closed namespace for the root Rust candidate compiler.
//!
//! Pure generation data. The candidate has its own namespace and fixed
//! homes, so no cached payload or caller-selected path can enter it.

use crate::catalog::rust_desktop::RustCompilerRole;
use crate::catalog::{RUST_VERSION, RustInstallOptions, rust_bootstrap::RustHost};

/// Fixed namespace for the root compiler candidate.
pub const ROOT_RUST_CANDIDATE_ROOT_EXPR: &str =
    "${{ runner.temp }}/velnor-control/root-rust-candidate";
/// Exact source-owned binding for the root compiler candidate.
pub const ROOT_RUST_CANDIDATE_ROOT_ENV: &str = "VELNOR_ROOT_RUST_CANDIDATE_ROOT";

/// Fixed leaves owned by the root compiler candidate installer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootRustCandidateLeaf {
    /// Fresh Cargo home used by the candidate compiler.
    CargoHome,
    /// Fresh Rustup home used by the candidate compiler.
    RustupHome,
    /// Verified Rustup installer scratch space.
    RustupBootstrap,
    /// Candidate native distribution staging directory.
    NativeDist,
    /// Directory containing the one verified candidate manager entrypoint.
    ManagerBin,
}

impl RootRustCandidateLeaf {
    /// All leaves owned by the root compiler candidate installer.
    pub const ALL: [Self; 5] = [
        Self::CargoHome,
        Self::RustupHome,
        Self::RustupBootstrap,
        Self::NativeDist,
        Self::ManagerBin,
    ];

    /// Fixed namespace-relative spelling; callers cannot provide a suffix.
    #[must_use]
    pub const fn relative(self) -> &'static str {
        match self {
            Self::CargoHome => "cargo-home",
            Self::RustupHome => "rustup-home",
            Self::RustupBootstrap => "rustup-bootstrap",
            Self::NativeDist => "native-dist",
            Self::ManagerBin => "manager-bin",
        }
    }
}

/// Immutable, nonserializable `RootLinux` candidate intent; no runtime power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootRustCandidateRoot {
    private: (),
}

impl RootRustCandidateRoot {
    /// Select the sole approved root compiler candidate purpose.
    #[must_use]
    pub const fn root_linux() -> Self {
        Self { private: () }
    }

    /// Compiled root environment binding for the candidate namespace.
    #[must_use]
    pub const fn namespace_environment(self) -> (&'static str, &'static str) {
        (ROOT_RUST_CANDIDATE_ROOT_ENV, ROOT_RUST_CANDIDATE_ROOT_EXPR)
    }

    /// Required concrete runner key; runtime capture owns path resolution.
    #[must_use]
    pub const fn runner_temp_environment(self) -> &'static str {
        "RUNNER_TEMP"
    }

    /// Fixed compiled location relative to runner temp.
    #[must_use]
    pub const fn relative_to_runner_temp(self) -> &'static str {
        "velnor-control/root-rust-candidate"
    }

    /// Fixed leaves owned by the candidate installer.
    #[must_use]
    pub const fn leaves(self) -> &'static [RootRustCandidateLeaf] {
        &RootRustCandidateLeaf::ALL
    }

    /// Exact root compiler pin, shared with catalog authority.
    #[must_use]
    pub const fn compiler_version(self) -> &'static str {
        RUST_VERSION
    }

    /// Sole approved host for the root candidate.
    #[must_use]
    pub const fn host(self) -> RustHost {
        RustCompilerRole::RootLinux.host()
    }

    /// Fixed `RootLinux` profile and components.
    #[must_use]
    pub fn options(self) -> RustInstallOptions {
        RustCompilerRole::RootLinux.options()
    }

    /// Expression form for one closed candidate leaf.
    #[must_use]
    pub fn leaf_expression(self, leaf: RootRustCandidateLeaf) -> String {
        format!("{ROOT_RUST_CANDIDATE_ROOT_EXPR}/{}", leaf.relative())
    }
}

#[cfg(test)]
#[path = "root_rust_candidate_root_tests.rs"]
mod tests;
