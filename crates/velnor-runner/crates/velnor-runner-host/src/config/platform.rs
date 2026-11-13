// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::ScaleSetSelector;

const SCALE_SET_SUFFIX: &str = "-scale-set";
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

    pub(super) fn routes(self, selector: &ScaleSetSelector) -> bool {
        let arm = selector.name().ends_with(ARM_SCALE_SET_SUFFIX);
        let foreign = |label: &String| match self {
            Self::LinuxAmd64 => label.ends_with(ARM_SCALE_SET_SUFFIX),
            Self::LinuxArm64 => {
                label.ends_with(SCALE_SET_SUFFIX) && !label.ends_with(ARM_SCALE_SET_SUFFIX)
            }
        };
        arm == (self == Self::LinuxArm64) && !selector.labels().iter().any(foreign)
    }
}
