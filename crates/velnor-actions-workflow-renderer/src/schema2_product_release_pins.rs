use velnor_actions_contract::{ReleaseTarget, Step};

use crate::setup::MiseSetup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductReleasePins {
    pub linux_x86_64_setup: MiseSetup,
    pub macos_arm64_setup: MiseSetup,
    pub macos_x86_64_setup: MiseSetup,
    pub install_gate_tools_argv: Vec<String>,
    pub install_build_tools_argv: Vec<String>,
    pub install_qualify_tools_argv: Vec<String>,
    pub qualify_mbx_action_steps: [Step; 3],
    pub install_runner_build_tools_argv: Vec<String>,
    pub install_gh_argv: Vec<String>,
    pub build_argv: Vec<String>,
    pub intel_build_argv: Vec<String>,
    pub install_intel_target_argv: Vec<String>,
    pub install_resource_probe_target_argv: Vec<String>,
    pub runner_build_argv: Vec<String>,
    pub resource_probe_build_argv: Vec<String>,
    pub install_resource_probe_release_tools_argv: Vec<String>,
    pub resource_probe_release_build_argv: Vec<String>,
    pub lint_argv: Vec<String>,
    pub verify_lint_argv: Vec<String>,
    pub gh_argv: Vec<String>,
    pub rust_version: String,
    pub verify_rust_argv: Vec<String>,
    pub mr_boxington_version: String,
    pub gh_version: String,
}

impl ProductReleasePins {
    #[must_use]
    pub fn setup_for(&self, target: ReleaseTarget) -> &MiseSetup {
        match target {
            ReleaseTarget::LinuxX86_64 => &self.linux_x86_64_setup,
            ReleaseTarget::MacosArm64 => &self.macos_arm64_setup,
            ReleaseTarget::MacosX86_64 => &self.macos_x86_64_setup,
        }
    }
}
