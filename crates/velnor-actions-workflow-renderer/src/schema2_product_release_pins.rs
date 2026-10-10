use velnor_actions_contract::ReleaseTarget;

use crate::setup::MiseSetup;

/// Pinned tools and runner-specific Mise setup for composed product releases.
///
/// The orchestrator builds every command vector through the Mise adapter.
/// The renderer only joins validated argv into workflow steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductReleasePins {
    /// Setup pins for each supported release runner target.
    pub linux_x86_64_setup: MiseSetup,
    /// Setup pins for each supported release runner target.
    pub macos_arm64_setup: MiseSetup,
    /// Setup pins for each supported release runner target.
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
    /// Exact `rustup target add` argv for the Intel target std.
    pub install_intel_target_argv: Vec<String>,
    /// Exact `rustup target add` argv for the probe's musl std.
    pub install_resource_probe_target_argv: Vec<String>,
    /// Exact pinned macOS host binary build argv.
    pub runner_build_argv: Vec<String>,
    /// Exact locked Cargo argv for the static Linux resource probe.
    pub resource_probe_build_argv: Vec<String>,
    /// Exact pinned actionlint argv.
    pub actionlint_argv: Vec<String>,
    /// Exact pinned zizmor argv.
    pub zizmor_argv: Vec<String>,
    /// Exact pinned GitHub CLI invocation prefix.
    pub gh_argv: Vec<String>,
    /// Exact Rust toolchain release selected by the Mise catalog.
    pub rust_version: String,
    /// Exact MBX release selected by the Mise catalog.
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
