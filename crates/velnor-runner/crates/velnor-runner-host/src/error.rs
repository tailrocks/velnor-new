//! Host failures. Text does not include tokens or JIT.

/// Cause of a failed pre-JIT `DinD` preparation whose owned resources are confirmed absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PreparationCause {
    /// The inner Docker API did not respond.
    #[error("Docker API failure")]
    Docker,
    /// A bounded Docker API call timed out.
    #[error("Docker API timeout")]
    DockerTimeout,
    /// The inner Docker API did not become ready.
    #[error("DinD readiness deadline")]
    DindReadiness,
    /// `DinD` did not use its private `VFS` data root.
    #[error("DinD storage mismatch")]
    DindStorage,
}

impl PreparationCause {
    /// Stable, bounded diagnostic label that excludes error contents.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::DockerTimeout => "docker_timeout",
            Self::DindReadiness => "dind_readiness",
            Self::DindStorage => "dind_storage",
        }
    }
}

/// Local controller failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    /// TOML was rejected.
    #[error("invalid config")]
    Config,
    /// Path was not absolute or not unicode.
    #[error("bad path")]
    Path,
    /// A second daemon already holds the lock.
    #[error("daemon lock held")]
    Lock,
    /// Journal read or write failed.
    #[error("journal")]
    Journal,
    /// Runner container plan violated the mount policy.
    #[error("forbidden mount")]
    ForbiddenMount,
    /// The runner container was marked privileged.
    #[error("privileged runner")]
    PrivilegedRunner,
    /// Frame exceeded the size bound.
    #[error("frame too large")]
    Frame,
    /// Docker client could not open the configured socket.
    #[error("docker socket")]
    Docker,
    /// A bounded Docker call did not return.
    #[error("docker operation timed out")]
    DockerTimeout,
    /// `DinD` did not expose its Docker API before the readiness deadline.
    #[error("DinD Docker API was not ready")]
    DindReadiness,
    /// `DinD` did not use the required private `VFS` data root.
    #[error("DinD storage configuration mismatch")]
    DindStorage,
    /// Pre-JIT preparation failed, and all exact local resources were removed.
    #[error("pre-JIT preparation failed with confirmed local cleanup: {0}")]
    PreparationFailedClean(PreparationCause),
    /// A Docker resource did not match its durable launch identity.
    #[error("Docker resource ownership mismatch")]
    Ownership,
    /// Cleanup found an active runner.
    #[error("runner is still active")]
    RunnerActive,
    /// Docker did not confirm resource cleanup.
    #[error("Docker resource cleanup failed")]
    Cleanup,
    /// A container create may have completed without a response.
    #[error("container create state is uncertain")]
    ContainerCreateUncertain,
    /// A container start may have completed without a response.
    #[error("container start state is uncertain")]
    ContainerStartUncertain,
    /// JIT delivery may have completed in part or in full.
    #[error("jit delivery state is uncertain")]
    JitDeliveryUncertain,
    /// The launch sequence exceeded its total deadline.
    #[error("launch state is uncertain after the deadline")]
    LaunchUncertain,
    /// A private-volume create may have completed without a response.
    #[error("private-volume create state is uncertain")]
    VolumeCreateUncertain,
    /// A collision-resistant worker ownership token could not be generated.
    #[error("worker identity")]
    Identity,
    /// JIT payload was empty. No container was created.
    #[error("empty jit")]
    EmptyJit,
    /// Endpoint was not an `https` URL.
    #[error("bad endpoint")]
    Endpoint,
    /// Keychain store failed, the read failed, or the token exceeded 4096 bytes.
    #[error("keychain")]
    Keychain,
    /// No token bytes were read.
    #[error("empty secret")]
    EmptySecret,
}

impl HostError {
    /// Stable, bounded diagnostic label that excludes error contents.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Path => "path",
            Self::Lock => "lock",
            Self::Journal => "journal",
            Self::ForbiddenMount => "forbidden_mount",
            Self::PrivilegedRunner => "privileged_runner",
            Self::Frame => "frame",
            Self::Docker => "docker",
            Self::DockerTimeout => "docker_timeout",
            Self::DindReadiness => "dind_readiness",
            Self::DindStorage => "dind_storage",
            Self::PreparationFailedClean(cause) => cause.code(),
            Self::Ownership => "ownership",
            Self::RunnerActive => "runner_active",
            Self::Cleanup => "cleanup",
            Self::ContainerCreateUncertain => "container_create_uncertain",
            Self::ContainerStartUncertain => "container_start_uncertain",
            Self::JitDeliveryUncertain => "jit_delivery_uncertain",
            Self::LaunchUncertain => "launch_uncertain",
            Self::VolumeCreateUncertain => "volume_create_uncertain",
            Self::Identity => "identity",
            Self::EmptyJit => "empty_jit",
            Self::Endpoint => "endpoint",
            Self::Keychain => "keychain",
            Self::EmptySecret => "empty_secret",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HostError, PreparationCause};

    #[test]
    fn host_error_codes_are_stable_and_bounded() {
        let cases = [
            (HostError::Config, "config"),
            (HostError::Path, "path"),
            (HostError::Lock, "lock"),
            (HostError::Journal, "journal"),
            (HostError::ForbiddenMount, "forbidden_mount"),
            (HostError::PrivilegedRunner, "privileged_runner"),
            (HostError::Frame, "frame"),
            (HostError::Docker, "docker"),
            (HostError::DockerTimeout, "docker_timeout"),
            (HostError::DindReadiness, "dind_readiness"),
            (HostError::DindStorage, "dind_storage"),
            (
                HostError::PreparationFailedClean(PreparationCause::DockerTimeout),
                "docker_timeout",
            ),
            (HostError::Ownership, "ownership"),
            (HostError::RunnerActive, "runner_active"),
            (HostError::Cleanup, "cleanup"),
            (
                HostError::ContainerCreateUncertain,
                "container_create_uncertain",
            ),
            (
                HostError::ContainerStartUncertain,
                "container_start_uncertain",
            ),
            (HostError::JitDeliveryUncertain, "jit_delivery_uncertain"),
            (HostError::LaunchUncertain, "launch_uncertain"),
            (HostError::VolumeCreateUncertain, "volume_create_uncertain"),
            (HostError::Identity, "identity"),
            (HostError::EmptyJit, "empty_jit"),
            (HostError::Endpoint, "endpoint"),
            (HostError::Keychain, "keychain"),
            (HostError::EmptySecret, "empty_secret"),
        ];
        for (error, expected) in cases {
            assert_eq!(error.code(), expected);
            assert!(error.code().len() <= 32);
        }
    }

    #[test]
    fn preparation_cause_codes_cover_every_cleanup_failure() {
        let cases = [
            (PreparationCause::Docker, "docker"),
            (PreparationCause::DockerTimeout, "docker_timeout"),
            (PreparationCause::DindReadiness, "dind_readiness"),
            (PreparationCause::DindStorage, "dind_storage"),
        ];
        for (cause, expected) in cases {
            assert_eq!(cause.code(), expected);
        }
    }
}
