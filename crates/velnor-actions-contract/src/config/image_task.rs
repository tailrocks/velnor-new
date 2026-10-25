use super::workflow_task::is_valid_workflow_task_id;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeImageTask {
    pub id: String,

    pub platform: NativeImagePlatform,

    pub script: String,

    pub timeout_minutes: u16,

    pub cache: NativeImageCachePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeImagePlatform {
    LinuxArm64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeImageCachePolicy {
    TaskOwnedBuilder,
}

impl NativeImagePlatform {
    #[must_use]
    pub const fn runs_on(self) -> &'static str {
        match self {
            Self::LinuxArm64 => "ubuntu-26.04-arm",
        }
    }

    #[must_use]
    pub const fn oci_platform(self) -> &'static str {
        match self {
            Self::LinuxArm64 => "linux/arm64",
        }
    }
}

impl NativeImageTask {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if !is_valid_workflow_task_id(&self.id) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.id",
                format!("bad_workflow_task_id:{}", self.id),
            ));
        }
        if !is_safe_native_image_script_path(&self.script) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.script",
                format!("bad_native_image_script:{}", self.script),
            ));
        }
        if !crate::workflow::timeout::JobTimeout::is_valid_minutes(u32::from(self.timeout_minutes))
        {
            return Err(ContractError::config(
                file,
                "workflow.tasks.timeout_minutes",
                format!("bad_native_image_timeout:{}", self.timeout_minutes),
            ));
        }
        Ok(())
    }
}

#[must_use]
fn is_safe_native_image_script_path(path: &str) -> bool {
    const PREFIX: &str = "maintained-image-build/";
    let Some(relative) = path.strip_prefix(PREFIX) else {
        return false;
    };
    let is_shell = std::path::Path::new(relative)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sh"));
    if !is_shell {
        return false;
    }
    relative.split('/').all(|component| {
        !component.is_empty()
            && component != "."
            && component != ".."
            && component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    })
}
