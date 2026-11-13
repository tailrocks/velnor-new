// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PreparationCause {
    #[error("Docker API failure")]
    Docker,

    #[error("Docker API timeout")]
    DockerTimeout,

    #[error("DinD readiness deadline")]
    DindReadiness,

    #[error("DinD storage mismatch")]
    DindStorage,
}

impl PreparationCause {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    #[error("invalid config")]
    Config,

    #[error("scale set selector does not match the runner platform")]
    PlatformSelector,

    #[error("bad path")]
    Path,

    #[error("daemon lock held")]
    Lock,

    #[error("journal")]
    Journal,

    #[error("forbidden mount")]
    ForbiddenMount,

    #[error("privileged runner")]
    PrivilegedRunner,

    #[error("frame too large")]
    Frame,

    #[error("docker socket")]
    Docker,

    #[error("docker operation timed out")]
    DockerTimeout,

    #[error("DinD Docker API was not ready")]
    DindReadiness,

    #[error("DinD storage configuration mismatch")]
    DindStorage,

    #[error("pre-JIT preparation failed with confirmed local cleanup: {0}")]
    PreparationFailedClean(PreparationCause),

    #[error("Docker resource ownership mismatch")]
    Ownership,

    #[error("runner is still active")]
    RunnerActive,

    #[error("Docker resource cleanup failed")]
    Cleanup,

    #[error("container create state is uncertain")]
    ContainerCreateUncertain,

    #[error("container start state is uncertain")]
    ContainerStartUncertain,

    #[error("jit delivery state is uncertain")]
    JitDeliveryUncertain,

    #[error("launch state is uncertain after the deadline")]
    LaunchUncertain,

    #[error("private-volume create state is uncertain")]
    VolumeCreateUncertain,

    #[error("worker identity")]
    Identity,

    #[error("empty jit")]
    EmptyJit,

    #[error("bad endpoint")]
    Endpoint,

    #[error("keychain")]
    Keychain,

    #[error("empty secret")]
    EmptySecret,
}

impl HostError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::PlatformSelector => "platform_selector",
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

impl From<velnor_runner_qualification::QualificationError> for HostError {
    fn from(error: velnor_runner_qualification::QualificationError) -> Self {
        use velnor_runner_qualification::QualificationError;
        match error {
            QualificationError::Docker => Self::Docker,
            QualificationError::DockerTimeout => Self::DockerTimeout,
            QualificationError::Identity => Self::Identity,
            QualificationError::Ownership => Self::Ownership,
            QualificationError::Cleanup => Self::Cleanup,
            QualificationError::ContainerCreateUncertain => Self::ContainerCreateUncertain,
            QualificationError::VolumeCreateUncertain => Self::VolumeCreateUncertain,
            QualificationError::DindReadiness => Self::DindReadiness,
            QualificationError::Frame => Self::Frame,
        }
    }
}
