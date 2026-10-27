// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MISE_BINARY_SHA256_LINUX_X64, MISE_VERSION};
use velnor_actions_contract::ReleaseTarget;

pub const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "7acfdbf412c8ec5b0d919d277417648c0534333d55f0242d5f9520d0d8ca7cc9";
pub const MISE_BINARY_SHA256_MACOS_X64: &str =
    "98b031e88eb0fd33c1507acb538f429747d26b770cd3cfe257408ec104ac9f6d";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiseBinaryIdentity {
    pub version: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

#[must_use]
pub fn current_mise_binary(target: ReleaseTarget) -> MiseBinaryIdentity {
    let (sha256, bytes) = match target {
        ReleaseTarget::LinuxX86_64 => (MISE_BINARY_SHA256_LINUX_X64, 175_451_280),
        ReleaseTarget::MacosArm64 => (MISE_BINARY_SHA256_MACOS_ARM64, 138_770_976),
        ReleaseTarget::MacosX86_64 => (MISE_BINARY_SHA256_MACOS_X64, 170_157_952),
    };
    MiseBinaryIdentity {
        version: MISE_VERSION,
        sha256,
        bytes,
    }
}
