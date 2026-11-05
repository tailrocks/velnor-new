// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MISE_BINARY_SHA256_LINUX_X64, MISE_VERSION};
use velnor_actions_contract::ReleaseTarget;

pub const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "8a3ef00765b009278197aba627ac00d6e9c63f6aeb04d4f4b2cfe0e59ddc2fd6";
pub const MISE_BINARY_SHA256_MACOS_X64: &str =
    "e06eb3f0311dedbf65952ec2c0247398848653b302e6aad99d7371ad9ba33301";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiseBinaryIdentity<'a> {
    pub version: &'a str,
    pub sha256: &'a str,
    pub bytes: u64,
}

#[must_use]
pub fn current_mise_binary(target: ReleaseTarget) -> MiseBinaryIdentity<'static> {
    let (sha256, bytes) = match target {
        ReleaseTarget::LinuxX86_64 => (MISE_BINARY_SHA256_LINUX_X64, 176_351_376),
        ReleaseTarget::MacosArm64 => (MISE_BINARY_SHA256_MACOS_ARM64, 140_146_464),
        ReleaseTarget::MacosX86_64 => (MISE_BINARY_SHA256_MACOS_X64, 171_256_016),
    };
    MiseBinaryIdentity {
        version: MISE_VERSION,
        sha256,
        bytes,
    }
}
