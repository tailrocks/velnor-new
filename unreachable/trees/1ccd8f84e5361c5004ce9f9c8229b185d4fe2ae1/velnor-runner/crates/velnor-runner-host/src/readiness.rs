//! Readiness is not "a process exists".

/// Controller readiness. `Ready` is set only by a bounded check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// Docker engine is down.
    WaitingForEngine,
    /// Keychain or gh credential is missing.
    WaitingForCredentials,
    /// Journal, Docker, and GitHub are being reconciled.
    Reconciling,
    /// Bounded readiness succeeded.
    Ready,
    /// New admissions are stopped.
    Draining,
    /// A required dependency failed after startup.
    Degraded,
}

impl Readiness {
    /// Stable JSON token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WaitingForEngine => "waiting_for_engine",
            Self::WaitingForCredentials => "waiting_for_credentials",
            Self::Reconciling => "reconciling",
            Self::Ready => "ready",
            Self::Draining => "draining",
            Self::Degraded => "degraded",
        }
    }
}

/// Status document. No token field.
#[must_use]
pub fn status_json(readiness: Readiness) -> String {
    format!(r#"{{"state":"{}"}}"#, readiness.as_str())
}

/// Doctor document. `probe` does not upgrade an unready state.
#[must_use]
pub fn doctor_json(readiness: Readiness, probe: bool) -> String {
    let state = readiness.as_str();
    format!(r#"{{"state":"{state}","probe":{probe}}}"#)
}

/// Empty controller directory is not ready.
#[must_use]
pub const fn readiness_for_empty() -> Readiness {
    Readiness::WaitingForCredentials
}
