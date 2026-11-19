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

const ERROR_DETAIL_BYTES: u8 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorDetail {
    text: [u8; ERROR_DETAIL_BYTES as usize],
    len: u8,
}

impl ErrorDetail {
    #[must_use]
    pub fn from_bytes(raw: &[u8]) -> Self {
        let mut text = [0_u8; ERROR_DETAIL_BYTES as usize];
        let mut len = 0_u8;
        let mut bytes = raw.iter().copied().peekable();
        let mut gap = false;
        while let Some(byte) = bytes.next() {
            if len == ERROR_DETAIL_BYTES {
                break;
            }
            if byte == 0x1b && bytes.peek() == Some(&b'[') {
                bytes.next();
                for next in bytes.by_ref() {
                    if (0x40..=0x7e).contains(&next) {
                        break;
                    }
                }
                continue;
            }
            if byte == b'\r' || byte == b'\n' {
                gap = len > 0;
                continue;
            }
            if !(byte.is_ascii_graphic() || byte == b' ') {
                continue;
            }
            if gap {
                text[usize::from(len)] = b' ';
                len += 1;
                gap = false;
                if len == ERROR_DETAIL_BYTES {
                    break;
                }
            }
            text[usize::from(len)] = byte;
            len += 1;
        }
        Self { text, len }
    }
}

impl std::fmt::Display for ErrorDetail {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&String::from_utf8_lossy(
            &self.text[..usize::from(self.len)],
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    #[error("invalid config")]
    Config,

    #[error("scale set selector does not match the runner platform")]
    PlatformSelector,

    #[error("developer images require a binary built without a release identity")]
    DeveloperImages,

    #[error("bad path")]
    Path,

    #[error("daemon lock held")]
    Lock,

    #[error(
        "control socket path {path} is {length} bytes but must be under the {limit}-byte limit; use a shorter --state folder"
    )]
    SocketPathTooLong {
        path: &'static str,
        length: usize,
        limit: usize,
    },

    #[error("cannot bind control socket {path}: {kind}")]
    SocketBind {
        path: &'static str,
        kind: std::io::ErrorKind,
    },

    #[error("journal")]
    Journal,

    #[error("worker helper failed: {0}")]
    Helper(ErrorDetail),

    #[error("poll failed: {0}")]
    Poll(ErrorDetail),

    #[error("forbidden mount")]
    ForbiddenMount,

    #[error("privileged runner")]
    PrivilegedRunner,

    #[error("frame too large")]
    Frame,

    #[error("docker socket")]
    Docker,

    #[error("runner container exited with a failure")]
    RunnerExited,

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

    #[error("a runner pair stayed in setup or teardown longer than the allowed time")]
    SettlingTimeout,

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
    pub fn poll(cause: &impl std::fmt::Display) -> Self {
        Self::Poll(ErrorDetail::from_bytes(cause.to_string().as_bytes()))
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::PlatformSelector => "platform_selector",
            Self::DeveloperImages => "developer_images",
            Self::Path => "path",
            Self::Lock => "lock",
            Self::SocketPathTooLong { .. } => "socket_path_too_long",
            Self::SocketBind { .. } => "socket_bind",
            Self::Journal => "journal",
            Self::Helper(_) => "helper_failed",
            Self::Poll(_) => "poll",
            Self::ForbiddenMount => "forbidden_mount",
            Self::PrivilegedRunner => "privileged_runner",
            Self::Frame => "frame",
            Self::Docker => "docker",
            Self::DockerTimeout => "docker_timeout",
            Self::DindReadiness => "dind_readiness",
            Self::DindStorage => "dind_storage",
            Self::PreparationFailedClean(cause) => cause.code(),
            Self::Ownership => "ownership",
            Self::SettlingTimeout => "settling_timeout",
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
            Self::RunnerExited => "runner_exited",
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
