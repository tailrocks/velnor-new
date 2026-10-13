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
