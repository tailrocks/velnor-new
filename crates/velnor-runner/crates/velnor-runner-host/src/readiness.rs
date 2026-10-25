#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    WaitingForEngine,

    WaitingForCredentials,

    Reconciling,

    Ready,

    Draining,

    Degraded,
}

impl Readiness {
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

#[must_use]
pub fn status_json(readiness: Readiness) -> String {
    format!(r#"{{"state":"{}"}}"#, readiness.as_str())
}

#[must_use]
pub fn doctor_json(readiness: Readiness, probe: bool) -> String {
    let state = readiness.as_str();
    format!(r#"{{"state":"{state}","probe":{probe}}}"#)
}

#[must_use]
pub const fn readiness_for_empty() -> Readiness {
    Readiness::WaitingForCredentials
}
