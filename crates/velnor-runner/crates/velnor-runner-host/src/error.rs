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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    #[error("invalid config")]
    Config,

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
