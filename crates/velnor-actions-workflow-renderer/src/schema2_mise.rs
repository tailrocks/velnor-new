//! Typed, target-specific Mise setup supplied by the orchestrator.

use velnor_actions_contract::{RunsOn, VerificationRunner, target_for_runner_label};

use crate::RenderError;
use crate::setup::MiseSetup;
use crate::yaml::Yaml;

/// Mise setup pins resolved by the owning orchestrator for supported runners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseSetupSet {
    /// Setup for Linux x64 runners.
    pub linux_x64: MiseSetup,
    /// Setup for macOS Apple ARM64 runners.
    pub macos_arm64: MiseSetup,
    /// Setup for macOS Intel x64 Phase0 qualification runners.
    pub macos_x64: MiseSetup,
}

impl MiseSetupSet {
    /// Validate all platform setup values before rendering any schema-2 workflow.
    /// # Errors
    pub fn validate(&self) -> Result<(), RenderError> {
        self.linux_x64.validate()?;
        self.macos_arm64.validate()?;
        self.macos_x64.validate()?;
        if self.linux_x64.version != self.macos_arm64.version
            || self.linux_x64.uses != self.macos_arm64.uses
            || self.linux_x64.version != self.macos_x64.version
            || self.linux_x64.uses != self.macos_x64.uses
        {
            return Err(RenderError::InvalidWorkflow(
                "schema2_mise_platform_pin_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    /// Return the typed setup for the requested platform.
    #[must_use]
    pub const fn for_runner(&self, runner: VerificationRunner) -> &MiseSetup {
        match runner {
            VerificationRunner::LinuxX64 => &self.linux_x64,
            VerificationRunner::MacosArm64 => &self.macos_arm64,
        }
    }

    /// Return the setup action digest matching one generated job's runner.
    ///
    /// Intel is a Mise executable platform for Phase0 only; this mapping
    /// does not extend Velnor's producer release-target set.
    #[must_use]
    pub fn for_runner_label(&self, label: &str) -> Option<&MiseSetup> {
        match RunsOn::parse(label).ok()? {
            RunsOn::ScaleSet(_) => Some(&self.linux_x64),
            RunsOn::Hosted(label) if label == "macos-15" => Some(&self.macos_arm64),
            RunsOn::Hosted(label) if label == "macos-15-intel" => Some(&self.macos_x64),
            RunsOn::Hosted(label)
                if target_for_runner_label(&label)
                    == Some(velnor_actions_contract::SUPPORTED_TARGETS[0]) =>
            {
                Some(&self.linux_x64)
            }
            RunsOn::Hosted(_) => None,
        }
    }
}

/// Render the single fixed setup-action form used by schema-2 release jobs.
pub(super) fn setup_step(setup: &MiseSetup) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Set up pinned Mise")),
        ("uses".to_owned(), Yaml::str(setup.uses.clone())),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("cache".to_owned(), Yaml::str("false")),
                ("env".to_owned(), Yaml::str("false")),
                ("install".to_owned(), Yaml::str("false")),
                ("sha256".to_owned(), Yaml::str(setup.sha256.clone())),
                ("version".to_owned(), Yaml::str(setup.version.clone())),
            ]),
        ),
    ])
}
