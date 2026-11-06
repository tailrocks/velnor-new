// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MISE_BINARY_SHA256_LINUX_X64, MISE_VERSION};
use velnor_actions_contract::ReleaseTarget;

pub const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "6d39119ab914c54ab865204dee9337c71b83a7ed1e730f663c2fc3453e38fe9c";
pub const MISE_BINARY_SHA256_MACOS_X64: &str =
    "49f32abd93f54147af0599a1c9583d2092df8e7250c9fe635783a7932c4087ef";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiseBinaryIdentity<'a> {
    pub version: &'a str,
    pub sha256: &'a str,
    pub bytes: u64,
}

#[must_use]
pub fn current_mise_binary(target: ReleaseTarget) -> MiseBinaryIdentity<'static> {
    let (sha256, bytes) = match target {
        ReleaseTarget::LinuxX86_64 => (MISE_BINARY_SHA256_LINUX_X64, 177_830_976),
        ReleaseTarget::MacosArm64 => (MISE_BINARY_SHA256_MACOS_ARM64, 141_305_312),
        ReleaseTarget::MacosX86_64 => (MISE_BINARY_SHA256_MACOS_X64, 172_898_672),
    };
    MiseBinaryIdentity {
        version: MISE_VERSION,
        sha256,
        bytes,
    }
}
