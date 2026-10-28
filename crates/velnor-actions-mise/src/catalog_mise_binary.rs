// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MISE_BINARY_SHA256_LINUX_X64, MISE_VERSION};
use velnor_actions_contract::ReleaseTarget;

pub const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "b20d30788a819fade398a1cb92aee56c383e3c34324162ec39fb8567dee79ba6";
pub const MISE_BINARY_SHA256_MACOS_X64: &str =
    "5dde6e3fcda4d8a68b95cc2180dacd58e7b940f86e4579991e13540a5d009055";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiseBinaryIdentity<'a> {
    pub version: &'a str,
    pub sha256: &'a str,
    pub bytes: u64,
}

#[must_use]
pub fn current_mise_binary(target: ReleaseTarget) -> MiseBinaryIdentity<'static> {
    let (sha256, bytes) = match target {
        ReleaseTarget::LinuxX86_64 => (MISE_BINARY_SHA256_LINUX_X64, 175_698_560),
        ReleaseTarget::MacosArm64 => (MISE_BINARY_SHA256_MACOS_ARM64, 138_821_904),
        ReleaseTarget::MacosX86_64 => (MISE_BINARY_SHA256_MACOS_X64, 169_964_688),
    };
    MiseBinaryIdentity {
        version: MISE_VERSION,
        sha256,
        bytes,
    }
}
