// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::ids::{CommitSha, HelperVersion, InvalidId, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperRelease {
    version: HelperVersion,
    action: CommitSha,
    digests: BTreeMap<Platform, Sha256>,
    binary_digests: Option<BTreeMap<Platform, Sha256>>,
}

impl HelperRelease {
    pub fn new(
        version: HelperVersion,
        action: CommitSha,
        digests: BTreeMap<Platform, Sha256>,
    ) -> Result<Self, InvalidId> {
        if digests.is_empty() {
            return Err(InvalidId {
                kind: "HelperRelease",
                value: version.to_string(),
            });
        }
        Ok(Self {
            version,
            action,
            digests,
            binary_digests: None,
        })
    }

    #[must_use]
    pub fn with_binary_digests(mut self, binary_digests: BTreeMap<Platform, Sha256>) -> Self {
        self.binary_digests = Some(binary_digests);
        self
    }

    #[must_use]
    pub fn version(&self) -> &HelperVersion {
        &self.version
    }

    #[must_use]
    pub fn action(&self) -> &CommitSha {
        &self.action
    }

    #[must_use]
    pub fn digests(&self) -> &BTreeMap<Platform, Sha256> {
        &self.digests
    }

    #[must_use]
    pub fn launcher_digests(&self) -> &BTreeMap<Platform, Sha256> {
        self.binary_digests.as_ref().unwrap_or(&self.digests)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Platform {
    LinuxX64,
    LinuxArm64,
    MacosArm64,
}

impl Platform {
    pub const ALL: [Self; 3] = [Self::LinuxX64, Self::LinuxArm64, Self::MacosArm64];
    pub const REQUIRED: [Self; 2] = [Self::LinuxX64, Self::LinuxArm64];

    #[must_use]
    pub const fn file_suffix(self) -> &'static str {
        match self {
            Self::LinuxX64 => "linux-x64",
            Self::LinuxArm64 => "linux-arm64",
            Self::MacosArm64 => "macos-arm64",
        }
    }

    #[must_use]
    pub const fn release_triple(self) -> &'static str {
        match self {
            Self::LinuxX64 => "x86_64-unknown-linux-musl",
            Self::LinuxArm64 => "aarch64-unknown-linux-musl",
            Self::MacosArm64 => "aarch64-apple-darwin",
        }
    }

    #[must_use]
    pub const fn runner_key(self) -> &'static str {
        match self {
            Self::LinuxX64 => "Linux-X64",
            Self::LinuxArm64 => "Linux-ARM64",
            Self::MacosArm64 => "macOS-ARM64",
        }
    }
}
