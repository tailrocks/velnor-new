// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CheckExecutor, CheckPlatform};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

#[path = "host_container_validation.rs"]
mod validation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostContainerProfile {
    Docker {
        context: String,

        socket_path: String,

        socket_uid: u32,

        cli: HostDockerCli,

        daemon: HostDockerDaemon,
    },

    OrbStack {
        context: String,

        socket_path: String,

        cli: HostDockerCli,

        daemon: HostDockerDaemon,

        sdk: Box<HostOrbStackSdk>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostDockerCli {
    pub path: String,

    pub sha256: String,

    pub version: String,

    pub build: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostDockerDaemon {
    pub version: String,

    pub platform: ContainerPlatform,

    pub operating_system: String,

    pub identity_policy: DaemonIdentityPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIdentityPolicy {
    ExecutionScoped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerPlatform {
    LinuxX64,

    LinuxArm64,
}

impl ContainerPlatform {
    #[must_use]
    pub const fn os(self) -> &'static str {
        "linux"
    }

    #[must_use]
    pub const fn arch(self) -> &'static str {
        match self {
            Self::LinuxX64 => "x86_64",
            Self::LinuxArm64 => "aarch64",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostOrbStackSdk {
    pub app_bundle_path: String,

    pub bundle_id: String,

    pub team_id: String,

    pub version: String,

    pub build: String,

    pub info_plist_sha256: String,

    pub main_executable_path: String,

    pub main_executable_sha256: String,

    pub cli_bundle_path: String,

    pub source_tree_sha256: String,

    pub owned_tree_sha256: String,

    pub cli_relative_path: String,

    pub cli_sha256: String,

    pub cli_version: String,

    pub cli_build: String,

    pub cli_commit: String,

    pub runtime_dir: String,

    pub runtime_uid: u32,
}

impl HostContainerProfile {
    #[must_use]
    pub fn context(&self) -> &str {
        match self {
            Self::Docker { context, .. } | Self::OrbStack { context, .. } => context,
        }
    }

    #[must_use]
    pub fn socket_path(&self) -> &str {
        match self {
            Self::Docker { socket_path, .. } | Self::OrbStack { socket_path, .. } => socket_path,
        }
    }

    #[must_use]
    pub fn socket_uid(&self) -> u32 {
        match self {
            Self::Docker { socket_uid, .. } => *socket_uid,
            Self::OrbStack { sdk, .. } => sdk.runtime_uid,
        }
    }

    #[must_use]
    pub const fn cli(&self) -> &HostDockerCli {
        match self {
            Self::Docker { cli, .. } | Self::OrbStack { cli, .. } => cli,
        }
    }

    #[must_use]
    pub const fn daemon(&self) -> &HostDockerDaemon {
        match self {
            Self::Docker { daemon, .. } | Self::OrbStack { daemon, .. } => daemon,
        }
    }

    pub fn validate(
        &self,
        platform: CheckPlatform,
        executor: CheckExecutor,
        file: &str,
        key: &str,
    ) -> Result<(), ContractError> {
        validation::validate(self, platform, executor, file, key)
    }
}
