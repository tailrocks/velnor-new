//! Pure Cargo build and closed native-proof requests for generator releases.

use std::ffi::OsString;
use std::path::PathBuf;

use velnor_actions_contract::{
    ContractError, GeneratorReleaseSourceBinding, GeneratorReleaseTarget, require_release_version,
};

/// One locked Cargo build request for the generator executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratorCargoBuild {
    target: GeneratorReleaseTarget,
}

/// Closed native checks derived from one matching Cargo build request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorBinaryCheck {
    /// Confirm the OS and machine match the selected native runner.
    NativeHostIdentity,
    /// Confirm the selected compiler release and host target.
    RustToolchainIdentity,
    /// Confirm executable format and exact architecture.
    BinaryFormatArchitecture,
    /// Confirm Linux shared-library requirements fit the fixed Ubuntu 22.04 ABI.
    GnuRuntimeAbi,
    /// Observe and validate the selected macOS SDK.
    AppleSdk,
    /// Observe and validate the selected Apple Clang and linker.
    AppleLinker,
    /// Execute the binary and require the exact package version.
    VersionSmoke,
    /// Execute the binary help path and require public usage entries.
    HelpSmoke,
}

/// Expected native proof facts derived from the release plan and Cargo request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorBinaryVerification {
    target: GeneratorReleaseTarget,
    version: String,
    rust_toolchain_version: String,
    binary_relative_path: PathBuf,
    checks: Vec<GeneratorBinaryCheck>,
}

impl GeneratorCargoBuild {
    /// Request a release build for one canonical target.
    #[must_use]
    pub const fn new(target: GeneratorReleaseTarget) -> Self {
        Self { target }
    }

    /// Cargo program to execute through the Mise adapter.
    #[must_use]
    pub const fn program(self) -> &'static str {
        "cargo"
    }

    /// Fixed Cargo arguments, including the explicit target and output root.
    #[must_use]
    pub fn args(self) -> Vec<OsString> {
        [
            "build",
            "--locked",
            "--release",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
            "--target-dir",
            "target",
            "--target",
            self.target.triple(),
        ]
        .into_iter()
        .map(OsString::from)
        .collect()
    }

    /// Repository-relative executable selected by the explicit target directory.
    #[must_use]
    pub fn binary_relative_path(self) -> PathBuf {
        PathBuf::from("target")
            .join(self.target.triple())
            .join("release")
            .join("velnor-actions")
    }

    /// Derive the complete platform proof inventory for this release candidate.
    ///
    /// The candidate version comes from the validated release plan. The toolchain
    /// version is separately validated as an exact numeric release.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] when `rust_toolchain_version` is not exact `X.Y.Z`.
    pub fn verification(
        self,
        plan: &GeneratorReleaseSourceBinding,
        rust_toolchain_version: &str,
    ) -> Result<GeneratorBinaryVerification, ContractError> {
        require_release_version(rust_toolchain_version, "rust-toolchain")?;
        let mut checks = vec![
            GeneratorBinaryCheck::NativeHostIdentity,
            GeneratorBinaryCheck::RustToolchainIdentity,
            GeneratorBinaryCheck::BinaryFormatArchitecture,
        ];
        match self.target {
            GeneratorReleaseTarget::LinuxX86_64 => checks.push(GeneratorBinaryCheck::GnuRuntimeAbi),
            GeneratorReleaseTarget::MacosArm64 => {
                checks.push(GeneratorBinaryCheck::AppleSdk);
                checks.push(GeneratorBinaryCheck::AppleLinker);
            }
        }
        checks.extend([
            GeneratorBinaryCheck::VersionSmoke,
            GeneratorBinaryCheck::HelpSmoke,
        ]);
        Ok(GeneratorBinaryVerification {
            target: self.target,
            version: plan.version().to_owned(),
            rust_toolchain_version: rust_toolchain_version.to_owned(),
            binary_relative_path: self.binary_relative_path(),
            checks,
        })
    }

    /// Target whose bytes the request produces.
    #[must_use]
    pub const fn target(self) -> GeneratorReleaseTarget {
        self.target
    }
}

impl GeneratorBinaryVerification {
    /// Target whose binary this request qualifies.
    #[must_use]
    pub const fn target(&self) -> GeneratorReleaseTarget {
        self.target
    }

    /// Exact package release version from the release plan.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Expected exact Rust compiler release.
    #[must_use]
    pub fn rust_toolchain_version(&self) -> &str {
        &self.rust_toolchain_version
    }

    /// Repository-relative binary output from the matching Cargo build request.
    #[must_use]
    pub fn binary_relative_path(&self) -> &std::path::Path {
        &self.binary_relative_path
    }

    /// Closed, target-specific native proof obligations.
    #[must_use]
    pub fn checks(&self) -> &[GeneratorBinaryCheck] {
        &self.checks
    }
}

#[cfg(test)]
#[path = "generator_release_build_tests.rs"]
mod tests;
