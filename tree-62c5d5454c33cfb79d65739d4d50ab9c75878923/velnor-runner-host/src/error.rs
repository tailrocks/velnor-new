//! Host failures. Text does not include tokens or JIT.

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
    /// Endpoint was not an `https` URL.
    #[error("bad endpoint")]
    Endpoint,
}
