//! Producer outcomes never replace repository obligation evidence.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    JobConclusion, Plan, ProducerAdmission, ProducerPolicy, ProducerRole, ProducerTerminalError,
};

/// Terminal observation from the exact generated producer's needs outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProducerReport {
    pub(crate) job_id: String,
    /// Generated source, executable descriptor, or computed MBX export identity.
    pub(crate) identity: String,
    pub(crate) verified: bool,
    pub(crate) cache_available: bool,
    pub(crate) error: ProducerTerminalError,
}

impl ProducerReport {
    /// Successful terminal observation; grants no payload or public-origin admission.
    pub(crate) fn terminal_success(
        &self,
        admission: &ProducerAdmission,
        conclusion: JobConclusion,
    ) -> bool {
        conclusion == JobConclusion::Success
            && self.matches(admission)
            && self.error == ProducerTerminalError::None
    }

    /// Bind terminal evidence to its declared role and coherent availability.
    pub(crate) fn matches(&self, admission: &ProducerAdmission) -> bool {
        let Ok(identity) = admission.identity() else {
            return false;
        };
        let source = match &admission.role {
            ProducerRole::Source { .. } => true,
            ProducerRole::Tool { .. } | ProducerRole::Mbx { .. } => false,
        };
        self.job_id == admission.job_id
            && self.identity == identity
            && terminal_consistent(self.verified, self.cache_available, self.error, source)
    }
}

fn terminal_consistent(
    verified: bool,
    available: bool,
    error: ProducerTerminalError,
    source: bool,
) -> bool {
    match error {
        ProducerTerminalError::None => verified && available,
        ProducerTerminalError::CacheNotPublished => verified && !available,
        ProducerTerminalError::CacheTransportFailed => verified,
        ProducerTerminalError::CacheTransportUnavailable => verified && !available,
        ProducerTerminalError::PreparationFailed => !verified && !available,
        ProducerTerminalError::PrivateOrAuthRequired
        | ProducerTerminalError::PublicAuthorityUnavailable
        | ProducerTerminalError::UnsupportedRegistry
        | ProducerTerminalError::SourceVerificationFailed => source && !verified && !available,
    }
}

/// Producer policy cannot exempt a job that owns any actual task obligation.
pub(crate) fn isolated(plan: &Plan, admission: &ProducerAdmission) -> bool {
    !plan
        .obligations
        .iter()
        .any(|ob| ob.job_id == admission.job_id)
        && !plan
            .matrix
            .include
            .iter()
            .any(|entry| entry.job_id == admission.job_id)
}

/// Cold fallback is explicit and requires a coherent terminal observation.
pub(crate) fn advisory_failure(
    admission: &ProducerAdmission,
    conclusion: JobConclusion,
    report: Option<&ProducerReport>,
) -> bool {
    conclusion == JobConclusion::Failure
        && admission.policy == ProducerPolicy::AdvisoryFallback
        && report.is_some_and(|report| report.matches(admission))
}

#[cfg(test)]
#[path = "required_producer_roles_tests.rs"]
mod tests;
