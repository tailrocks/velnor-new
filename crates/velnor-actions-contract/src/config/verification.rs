use super::{MiseTaskSource, mise::is_valid_mise_task_name};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationTask {
    pub id: String,

    pub mise_task: String,

    pub source: MiseTaskSource,

    pub runner: VerificationRunner,

    pub timeout_minutes: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationRunner {
    LinuxX64,

    MacosArm64,

    #[serde(rename = "macos-26-arm64")]
    Macos26Arm64,

    #[serde(rename = "xcode-27-arm64")]
    Xcode27Arm64,
}

impl VerificationRunner {
    #[must_use]
    pub const fn runs_on(self) -> &'static str {
        match self {
            Self::LinuxX64 => "ubuntu-26.04",
            Self::MacosArm64 => "macos-15",
            Self::Macos26Arm64 => "macos-26",
            Self::Xcode27Arm64 => "xcode-27",
        }
    }

    #[must_use]
    pub const fn mise_target(self) -> &'static str {
        match self {
            Self::LinuxX64 => "x86_64-unknown-linux-gnu",
            Self::MacosArm64 | Self::Macos26Arm64 | Self::Xcode27Arm64 => "aarch64-apple-darwin",
        }
    }
}

impl VerificationTask {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if !super::is_valid_workflow_task_id(&self.id) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.id",
                format!("bad_workflow_task_id:{}", self.id),
            ));
        }
        if !is_valid_mise_task_name(&self.mise_task) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.mise_task",
                format!("bad_mise_task:{}", self.mise_task),
            ));
        }
        if !self.source.validate() {
            return Err(ContractError::config(
                file,
                "workflow.tasks.source",
                "bad_verification_source_or_working_directory",
            ));
        }
        if !crate::workflow::timeout::JobTimeout::is_valid_minutes(u32::from(self.timeout_minutes))
        {
            return Err(ContractError::config(
                file,
                "workflow.tasks.timeout_minutes",
                format!("bad_timeout:{}", self.timeout_minutes),
            ));
        }
        Ok(())
    }
}
