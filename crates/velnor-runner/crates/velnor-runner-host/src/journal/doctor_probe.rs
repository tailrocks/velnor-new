//! Doctor probe request records for idempotent requests.

mod api;
mod lookup;
#[cfg(test)]
mod lookup_tests;
mod recovery;
#[cfg(test)]
mod tests;

pub(super) use recovery::recover_at_startup;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoctorProbeState {
    Queued,
    Running,
    Recovering,
    Uncertain,
    Failed,
    Completed,
    Stale,
}

impl DoctorProbeState {
    pub(super) fn parse(value: &str) -> Result<Self, crate::error::HostError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "recovering" => Ok(Self::Recovering),
            "uncertain" => Ok(Self::Uncertain),
            "failed" => Ok(Self::Failed),
            "completed" => Ok(Self::Completed),
            "stale" => Ok(Self::Stale),
            _ => Err(crate::error::HostError::Journal),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Recovering => "recovering",
            Self::Uncertain => "uncertain",
            Self::Failed => "failed",
            Self::Completed => "completed",
            Self::Stale => "stale",
        }
    }

    pub(crate) const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Running | Self::Recovering | Self::Uncertain
        )
    }
}

pub(crate) struct DoctorProbeTicket {
    request_id: i64,
    nonce: String,
}

impl DoctorProbeTicket {
    pub(crate) const fn request_id(&self) -> i64 {
        self.request_id
    }

    pub(crate) fn nonce(&self) -> &str {
        &self.nonce
    }
}

pub(crate) enum DoctorProbeStartResult {
    Created(DoctorProbeTicket),
    Replayed(DoctorProbeTicket),
    Busy { active_request_id: i64 },
}

pub(crate) enum DoctorProbePrepareResult {
    Prepared(crate::journal::ProbeRow),
    AlreadyLinked(crate::journal::ProbeRow),
    WaitingForProbe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoctorProbeOutcome {
    Failed,
    Completed,
    Stale,
}

pub(crate) struct DoctorProbeRequestRecord {
    request_id: i64,
    nonce: String,
    state: DoctorProbeState,
    child: Option<crate::journal::ProbeRow>,
}

impl DoctorProbeRequestRecord {
    pub(crate) const fn request_id(&self) -> i64 {
        self.request_id
    }

    pub(crate) const fn state(&self) -> DoctorProbeState {
        self.state
    }

    pub(crate) fn child(&self) -> Option<&crate::journal::ProbeRow> {
        self.child.as_ref()
    }

    pub(crate) fn ticket(&self) -> DoctorProbeTicket {
        DoctorProbeTicket {
            request_id: self.request_id,
            nonce: self.nonce.clone(),
        }
    }
}
