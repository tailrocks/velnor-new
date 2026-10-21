//! This module records retirement claims after JIT configuration delivery.

#[cfg(test)]
mod candidate_tests;
mod claim;
#[cfg(test)]
mod claim_tests;
mod disposition;
#[cfg(test)]
mod effect_fence_tests;
mod identity;
mod local;
#[cfg(test)]
mod producer_tests;
mod progress;
mod remote;
mod remote_prepare;
#[cfg(test)]
#[path = "post_jit_retirement/test_support.rs"]
pub(crate) mod test_support;

pub(crate) use claim::{PostJitRetirementCandidate, PostJitRetirementClaim};
pub(in crate::journal) use disposition::{EventRetirementDisposition, EventRetirementStatus};
pub(crate) use remote_prepare::PreparedRetirementRemoteAbsence;

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

impl ConnectionJournal {
    /// Create a retirement claim for one physical attempt with a specified time limit.
    #[cfg(test)]
    pub(crate) async fn claim_post_jit_retirement(
        &self,
        reservation_id: i64,
        lease_seconds: i64,
    ) -> Result<Option<PostJitRetirementClaim>, HostError> {
        claim::claim(self, reservation_id, lease_seconds).await
    }

    #[cfg(test)]
    pub(crate) async fn persisted_post_jit_retirement_claim_for_test(
        &self,
        reservation_id: i64,
    ) -> Result<PostJitRetirementClaim, HostError> {
        crate::journal::connection::storage::read_transaction(
            self.journal_ref(),
            async |connection| {
                claim::persisted_claim_for_test(connection, self, reservation_id).await
            },
        )
        .await
    }

    /// List exact recovering attempts eligible for a retirement claim in this connection scope.
    pub(crate) async fn recoverable_post_jit_retirements(
        &self,
    ) -> Result<Vec<claim::PostJitRetirementCandidate>, HostError> {
        claim::recoverable_candidates(self).await
    }

    /// Claim a candidate after rechecking its identity and recovery state in one transaction.
    pub(crate) async fn claim_recoverable_post_jit_retirement(
        &self,
        candidate: &claim::PostJitRetirementCandidate,
        lease_seconds: i64,
    ) -> Result<Option<PostJitRetirementClaim>, HostError> {
        claim::claim_candidate(self, candidate, lease_seconds).await
    }

    /// Check whether this retirement claim is current and has not expired.
    pub(crate) async fn validate_post_jit_retirement_claim(
        &self,
        claim: &PostJitRetirementClaim,
    ) -> Result<(), HostError> {
        progress::validate_claim(self, claim).await
    }

    /// Check recorded network removal against this claim and reuse it for the newer claim generation.
    pub(crate) async fn resume_retirement_network_absence(
        &self,
        claim: &PostJitRetirementClaim,
    ) -> Result<bool, HostError> {
        progress::resume_network_absence(self, claim).await
    }

    /// Record observed local cleanup before remote verification.
    pub(crate) async fn record_retirement_local_absence(
        &self,
        claim: &PostJitRetirementClaim,
        proof: crate::launch::bootstrap::VerifiedStorageCleanup,
    ) -> Result<(), HostError> {
        local::record_local_absence(self, claim, proof).await
    }

    /// Check the stored local cleanup evidence again and reuse it for this retirement claim.
    pub(crate) async fn resume_retirement_local_absence(
        &self,
        claim: &PostJitRetirementClaim,
    ) -> Result<bool, HostError> {
        local::resume_local_absence(self, claim).await
    }

    /// Persist a validated remote runner absence receipt.
    pub(crate) async fn record_retirement_remote_absence(
        &self,
        claim: &PostJitRetirementClaim,
        absence: &crate::launch::retirement::AuthenticatedRetirementAbsence,
    ) -> Result<(), HostError> {
        remote::record_remote_absence(self, claim, absence).await
    }

    /// Persist a mocked receipt for journal-storage tests only.
    #[cfg(test)]
    pub(crate) async fn record_mocked_retirement_remote_absence(
        &self,
        claim: &PostJitRetirementClaim,
        receipt: &velnor_runner_github::RunnerAbsenceReceipt,
    ) -> Result<(), HostError> {
        remote::record_mocked_remote_absence(self, claim, receipt).await
    }

    /// Check the stored local storage cleanup evidence again before recording the next retirement phase.
    pub(crate) async fn record_retirement_storage_phase(
        &self,
        claim: &PostJitRetirementClaim,
    ) -> Result<(), HostError> {
        local::record_storage_phase(self, claim).await
    }
}

pub(in crate::journal) async fn record_network_phase_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    progress::record_network_phase_in_transaction(connection, scope, claim).await
}

pub(in crate::journal) async fn validate_claim_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    progress::validate_in_transaction(connection, scope, claim).await
}

pub(in crate::journal) async fn disposition_for_event_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    scale_set_id: i64,
    actual_request_id: i64,
    runner_id: i64,
    runner_name: &str,
) -> Result<Option<EventRetirementDisposition>, HostError> {
    disposition::for_event(
        connection,
        scope,
        scale_set_id,
        actual_request_id,
        runner_id,
        runner_name,
    )
    .await
}
