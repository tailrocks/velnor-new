// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MISE_BINARY_SHA256_LINUX_X64, MISE_VERSION};
use velnor_actions_contract::ReleaseTarget;

pub const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "9ca1005930fed90e0f4c30d9b13fe4530fe88d0549a71349716a3aab9051c627";
pub const MISE_BINARY_SHA256_MACOS_X64: &str =
    "ead2414251e8f2909cc7564f8ab5fd1feea6962ed77d0177776d72e08ae747a0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiseBinaryIdentity<'a> {
    pub version: &'a str,
    pub sha256: &'a str,
    pub bytes: u64,
}

#[must_use]
pub fn current_mise_binary(target: ReleaseTarget) -> MiseBinaryIdentity<'static> {
    let (sha256, bytes) = match target {
        ReleaseTarget::LinuxX86_64 => (MISE_BINARY_SHA256_LINUX_X64, 177_907_728),
        ReleaseTarget::MacosArm64 => (MISE_BINARY_SHA256_MACOS_ARM64, 141_426_624),
        ReleaseTarget::MacosX86_64 => (MISE_BINARY_SHA256_MACOS_X64, 172_294_368),
    };
    MiseBinaryIdentity {
        version: MISE_VERSION,
        sha256,
        bytes,
    }
}
