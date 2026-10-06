//! Explicit installed host container profiles; acquisition never loads host settings.
use super::{CheckExecutor, CheckPlatform};
use serde::{Deserialize, Serialize};
use velnor_actions_contract::errors::ContractError;

mod validation;

/// Fully declared installed Docker host runtime, independent of native target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostContainerProfile {
    /// Explicit Docker CLI and local engine installation.
    Docker {
        /// Explicit context whose local socket is verified.
        context: String,
        /// Explicit canonical absolute Unix socket; context metadata cannot override it.
        socket_path: String,
        /// Exact Unix socket owner UID; root-owned Docker sockets use zero.
        socket_uid: u32,
        /// Installed CLI byte and release identity.
        cli: HostDockerCli,
        /// Expected engine identity, separate from native runner platform.
        daemon: HostDockerDaemon,
    },
    /// Explicit signed `OrbStack` app and retained nested CLI SDK.
    OrbStack {
        /// Explicit context whose local `OrbStack` socket is verified.
        context: String,
        /// Explicit canonical absolute Unix socket; context metadata cannot override it.
        socket_path: String,
        /// Installed Docker CLI byte and release identity.
        cli: HostDockerCli,
        /// Expected `OrbStack` Docker engine identity.
        daemon: HostDockerDaemon,
        /// Signed app and nested CLI SDK qualification.
        sdk: Box<HostOrbStackSdk>,
    },
}

/// Existing canonical Docker CLI selected without ambient PATH lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostDockerCli {
    /// Canonical absolute executable path; runtime rejects links and drift.
    pub path: String,
    /// SHA-256 of installed executable bytes.
    pub sha256: String,
    /// Exact fixed `docker --version` release version.
    pub version: String,
    /// Exact fixed `docker --version` build identifier.
    pub build: String,
}

/// Exact local Docker engine identity returned by a fixed readonly JSON probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostDockerDaemon {
    /// Exact server release version.
    pub version: String,
    /// Explicit engine OS and architecture; never inferred from native target.
    pub platform: ContainerPlatform,
    /// Exact reported operating system description, including `OrbStack`.
    pub operating_system: String,
    /// Daemon ID is captured and remains exact throughout this execution.
    pub identity_policy: DaemonIdentityPolicy,
}

/// Required execution-scoped daemon continuity; no external result reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIdentityPolicy {
    /// Capture nonempty initial ID; require exact pre/post identity and receipt binding.
    ExecutionScoped,
}

/// Supported Linux engine platforms, distinct from the host runner platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerPlatform {
    /// Linux engine on x86-64.
    LinuxX64,
    /// Linux engine on ARM64.
    LinuxArm64,
}

impl ContainerPlatform {
    /// Exact engine operating system.
    #[must_use]
    pub const fn os(self) -> &'static str {
        "linux"
    }
    /// Canonical engine architecture.
    #[must_use]
    pub const fn arch(self) -> &'static str {
        match self {
            Self::LinuxX64 => "x86_64",
            Self::LinuxArm64 => "aarch64",
        }
    }
}

/// Existing signed `OrbStack` app and independently qualified nested CLI bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostOrbStackSdk {
    /// Canonical installed outer `.app` bundle path.
    pub app_bundle_path: String,
    /// Expected outer app bundle identity.
    pub bundle_id: String,
    /// Expected Apple Developer Team ID for both signed app and nested CLI.
    pub team_id: String,
    /// Exact outer app short version; distinct from nested plist version.
    pub version: String,
    /// Exact outer app build; distinct from CLI-reported build.
    pub build: String,
    /// SHA-256 of outer app `Contents/Info.plist` bytes.
    pub info_plist_sha256: String,
    /// Main executable path relative to the outer app bundle.
    pub main_executable_path: String,
    /// SHA-256 of outer app main executable bytes.
    pub main_executable_sha256: String,
    /// Canonical signed nested CLI `.app` path, contained by the outer app.
    pub cli_bundle_path: String,
    /// SHA-256 of original nested CLI bundle tree, before readonly projection.
    /// Canonical sorted JSON entries bind path/kind and full mode (`mode & 0o777`),
    /// file SHA-256 plus executable flag, or contained symlink target; root omitted.
    pub source_tree_sha256: String,
    /// Same tree recipe after retained copy and readonly mode normalization.
    pub owned_tree_sha256: String,
    /// Executable path relative to the nested CLI bundle.
    pub cli_relative_path: String,
    /// SHA-256 of nested CLI executable bytes.
    pub cli_sha256: String,
    /// Exact version from fixed CLI `version` probe.
    pub cli_version: String,
    /// Exact CLI-reported numeric build, independent of outer app build.
    pub cli_build: String,
    /// Exact CLI-reported 40-byte lowercase commit SHA.
    pub cli_commit: String,
    /// Explicit canonical absolute host `.orbstack/run` socket directory.
    pub runtime_dir: String,
    /// Exact owner UID of the declared `OrbStack` runtime directory and sockets.
    pub runtime_uid: u32,
}

impl HostContainerProfile {
    /// Context selection, shared by both supported providers.
    #[must_use]
    pub fn context(&self) -> &str {
        match self {
            Self::Docker { context, .. } | Self::OrbStack { context, .. } => context,
        }
    }
    /// Declared local Unix socket endpoint.
    #[must_use]
    pub fn socket_path(&self) -> &str {
        match self {
            Self::Docker { socket_path, .. } | Self::OrbStack { socket_path, .. } => socket_path,
        }
    }
    /// Exact declared socket owner; Docker root ownership is supported.
    #[must_use]
    pub fn socket_uid(&self) -> u32 {
        match self {
            Self::Docker { socket_uid, .. } => *socket_uid,
            Self::OrbStack { sdk, .. } => sdk.runtime_uid,
        }
    }
    /// Installed Docker CLI pin.
    #[must_use]
    pub const fn cli(&self) -> &HostDockerCli {
        match self {
            Self::Docker { cli, .. } | Self::OrbStack { cli, .. } => cli,
        }
    }
    /// Explicit expected engine identity.
    #[must_use]
    pub const fn daemon(&self) -> &HostDockerDaemon {
        match self {
            Self::Docker { daemon, .. } | Self::OrbStack { daemon, .. } => daemon,
        }
    }
    /// Validate the installed profile and native placement.
    /// # Errors
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

#[cfg(test)]
mod tests;
