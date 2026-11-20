// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum QualificationError {
    #[error("Docker API failure")]
    Docker,
    #[error("Docker API timeout")]
    DockerTimeout,
    #[error("invalid identity")]
    Identity,
    #[error("resource ownership mismatch")]
    Ownership,
    #[error("resource cleanup failed")]
    Cleanup,
    #[error("container create state uncertain")]
    ContainerCreateUncertain,
    #[error("volume create state uncertain")]
    VolumeCreateUncertain,
    #[error("DinD readiness deadline")]
    DindReadiness,
    #[error("probe frame invalid")]
    Frame,
}
