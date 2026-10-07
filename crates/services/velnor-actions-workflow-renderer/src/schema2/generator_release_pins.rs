use velnor_actions_contract_release::ReleaseTarget;

use velnor_actions_workflow_steps::setup::MiseSetup;

/// Pinned tools and runner-specific Mise setup for the generator release.
///
/// The orchestrator builds every command vector through the Mise adapter.
/// The renderer only joins validated argv into workflow steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorReleasePins {
    /// Setup pins for the Linux `x86_64` release runner.
    pub linux_x86_64_setup: MiseSetup,
    /// Setup pins for the macOS arm64 release runner.
    pub macos_arm64_setup: MiseSetup,
    /// Setup pins for the macOS `x86_64` release runner.
    pub macos_x86_64_setup: MiseSetup,
    /// Exact `mise install` argv for source-policy tools.
    pub install_gate_tools_argv: Vec<String>,
    /// Exact `mise install` argv for native candidate build tools.
    pub install_build_tools_argv: Vec<String>,
    /// Exact `mise install` argv for GitHub CLI.
    pub install_gh_argv: Vec<String>,
    /// Exact pinned `mbx build` argv.
    pub build_argv: Vec<String>,
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

impl GeneratorReleasePins {
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
