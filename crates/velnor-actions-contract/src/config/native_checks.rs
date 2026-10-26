// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ContractError,
    config::{CheckExecutor, CheckPlatform, CheckRunner},
    package_release::PackageReleaseChannel,
    workflow::timeout::JobTimeout,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHomebrewCheck {
    pub id: String,

    pub operation: NativeHomebrewOperation,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula_channel: Option<PackageReleaseChannel>,

    pub runner: CheckRunner,

    #[serde(default = "default_timeout")]
    pub timeout_minutes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeHomebrewOperation {
    TapTree,

    FormulaContract,

    Install,

    AdoptionContract,
}

impl NativeHomebrewOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TapTree => "tap_tree",
            Self::FormulaContract => "formula_contract",
            Self::Install => "install",
            Self::AdoptionContract => "adoption_contract",
        }
    }
}

const fn default_timeout() -> u32 {
    5
}

impl NativeHomebrewCheck {
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        let bad = |field, problem| ContractError::config(file, format!("{key}.{field}"), problem);
        if self.id.is_empty()
            || self.id.len() > 128
            || !self.id.as_bytes()[0].is_ascii_alphanumeric() && self.id.as_bytes()[0] != b'_'
            || !self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(bad("id", "bad_native_check_id"));
        }
        let channel_required = matches!(
            self.operation,
            NativeHomebrewOperation::FormulaContract | NativeHomebrewOperation::Install
        );
        if channel_required != self.formula_channel.is_some() {
            return Err(bad("formula_channel", "operation_channel_mismatch"));
        }
        if self.runner.executor != CheckExecutor::Hosted {
            return Err(bad(
                "runner.executor",
                "native_homebrew_requires_hosted_runner",
            ));
        }
        if self.operation == NativeHomebrewOperation::Install
            && self.runner.platform != CheckPlatform::MacosArm64
        {
            return Err(bad(
                "runner.platform",
                "native_homebrew_install_requires_macos_arm64",
            ));
        }
        let (minimum, maximum) = match self.operation {
            NativeHomebrewOperation::TapTree
            | NativeHomebrewOperation::FormulaContract
            | NativeHomebrewOperation::AdoptionContract => (1, 5),
            NativeHomebrewOperation::Install => (1, 30),
        };
        if !(minimum..=maximum).contains(&self.timeout_minutes)
            || !JobTimeout::is_valid_minutes(self.timeout_minutes)
        {
            return Err(bad("timeout_minutes", "invalid_native_check_timeout"));
        }
        self.runner.validate(file, &format!("{key}.runner"))?;
        Ok(())
    }
}
