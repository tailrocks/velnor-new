//! Typed tool and runner inputs for the composed product-release workflow.

use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_workflow_steps::setup::MiseSetup;

/// Orchestrator-resolved pins consumed by the composed product-release route.
///
/// The existing family workflows still receive [`GeneratorReleasePins`]
/// until their composition is wired. This type carries their shared inputs
/// alongside the additional typed commands required by the coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductReleasePins {
    /// Setup pins for the Linux `x86_64` runner.
    pub linux_x86_64_setup: MiseSetup,
    /// Setup pins for the macOS arm64 runner.
    pub macos_arm64_setup: MiseSetup,
    /// Setup pins for the macOS `x86_64` runner.
    pub macos_x86_64_setup: MiseSetup,
    /// Exact `mise install` argv for source-policy tools.
    pub install_gate_tools_argv: Vec<String>,
    /// Exact `mise install` argv for native candidate build tools.
    pub install_build_tools_argv: Vec<String>,
    /// Exact `mise install` argv for candidate qualification tools.
    pub install_qualify_tools_argv: Vec<String>,
    /// Exact `mise install` argv for the macOS host binary build.
    pub install_runner_build_tools_argv: Vec<String>,
    /// Exact `mise install` argv for GitHub CLI.
    pub install_gh_argv: Vec<String>,
    /// Exact pinned `mbx build` argv.
    pub build_argv: Vec<String>,
    /// Exact pinned `mbx build` argv for the Intel cross-compile leg.
    pub intel_build_argv: Vec<String>,
    /// Exact typed Rust target-install argv for the Intel cross-compile leg.
    pub install_intel_target_argv: Vec<String>,
    /// Exact pinned macOS host binary build argv.
    pub runner_build_argv: Vec<String>,
    /// Exact pinned actionlint argv.
    pub actionlint_argv: Vec<String>,
    /// Exact pinned zizmor argv with ambient credential variables removed.
    pub zizmor_argv: Vec<String>,
    /// Exact pinned GitHub CLI invocation prefix.
    pub gh_argv: Vec<String>,
    /// Exact Rust toolchain version selected by the Mise catalog.
    pub rust_version: String,
    /// Exact MBX version selected by the Mise catalog.
    pub mr_boxington_version: String,
}

impl ProductReleasePins {
    /// Setup action pins associated with a release target.
    #[must_use]
    pub fn setup_for(&self, target: ReleaseTarget) -> &MiseSetup {
        match target {
            ReleaseTarget::LinuxX86_64 => &self.linux_x86_64_setup,
            ReleaseTarget::MacosArm64 => &self.macos_arm64_setup,
            ReleaseTarget::MacosX86_64 => &self.macos_x86_64_setup,
        }
    }
}

#[cfg(test)]
#[path = "product_release_pins/tests.rs"]
mod tests;
