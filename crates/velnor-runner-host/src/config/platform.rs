// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::ScaleSetSelector;

const VELNOR_LABEL: &str = "velnor";
const ARM_MARKER: &str = "arm-scale-set";
const ARM_SCALE_SET_SUFFIX: &str = "-arm-scale-set";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerPlatform {
    LinuxAmd64,
    LinuxArm64,
}

impl RunnerPlatform {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "linux/amd64" => Some(Self::LinuxAmd64),
            "linux/arm64" => Some(Self::LinuxArm64),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "linux/amd64",
            Self::LinuxArm64 => "linux/arm64",
        }
    }

    #[must_use]
    pub const fn daemon_architecture(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "x86_64",
            Self::LinuxArm64 => "aarch64",
        }
    }

    #[must_use]
    pub fn from_image_architecture(value: &str) -> Option<Self> {
        match value {
            "amd64" => Some(Self::LinuxAmd64),
            "arm64" => Some(Self::LinuxArm64),
            _ => None,
        }
    }

    #[must_use]
    pub const fn image_architecture(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "amd64",
            Self::LinuxArm64 => "arm64",
        }
    }

    pub(super) fn routes(self, selector: &ScaleSetSelector) -> bool {
        match self {
            Self::LinuxAmd64 => !selector
                .labels()
                .iter()
                .any(|label| label.contains(ARM_MARKER)),
            Self::LinuxArm64 => {
                arm_selector(selector.name())
                    && selector
                        .labels()
                        .iter()
                        .all(|label| label == VELNOR_LABEL || arm_selector(label))
            }
        }
    }
}

fn arm_selector(label: &str) -> bool {
    label
        .strip_suffix(ARM_SCALE_SET_SUFFIX)
        .and_then(|image| image.split_once('-'))
        .is_some_and(|(os, version)| {
            !os.is_empty()
                && os.bytes().all(|byte| byte.is_ascii_lowercase())
                && version
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        })
}
