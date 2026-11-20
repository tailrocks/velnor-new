// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod claim;

mod disposition;

mod identity;
mod local;

mod progress;
mod remote;
mod remote_prepare;

pub(crate) use claim::{PostJitRetirementCandidate, PostJitRetirementClaim};
pub(in crate::journal) use disposition::{EventRetirementDisposition, EventRetirementStatus};
pub(crate) use remote_prepare::PreparedRetirementRemoteAbsence;

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

impl ConnectionJournal {
    pub(crate) async fn recoverable_post_jit_retirements(
        &self,
    ) -> Result<Vec<claim::PostJitRetirementCandidate>, HostError> {
        claim::recoverable_candidates(self).await
    }

    pub(crate) async fn claim_recoverable_post_jit_retirement(
        &self,
        candidate: &claim::PostJitRetirementCandidate,
        lease_seconds: i64,
    ) -> Result<Option<PostJitRetirementClaim>, HostError> {
        claim::claim_candidate(self, candidate, lease_seconds).await
    }

    pub(crate) async fn validate_post_jit_retirement_claim(
        &self,
        claim: &PostJitRetirementClaim,
    ) -> Result<(), HostError> {
        progress::validate_claim(self, claim).await
    }

    pub(crate) async fn resume_retirement_network_absence(
        &self,
        claim: &PostJitRetirementClaim,
    ) -> Result<bool, HostError> {
        progress::resume_network_absence(self, claim).await
    }

    pub(crate) async fn record_retirement_local_absence(
        &self,
        claim: &PostJitRetirementClaim,
        proof: crate::launch::bootstrap::VerifiedStorageCleanup,
    ) -> Result<(), HostError> {
        local::record_local_absence(self, claim, proof).await
    }

    pub(crate) async fn resume_retirement_local_absence(
        &self,
        claim: &PostJitRetirementClaim,
    ) -> Result<bool, HostError> {
        local::resume_local_absence(self, claim).await
    }

    pub(crate) async fn record_retirement_remote_absence(
        &self,
        claim: &PostJitRetirementClaim,
        absence: &crate::launch::retirement::AuthenticatedRetirementAbsence,
    ) -> Result<(), HostError> {
        remote::record_remote_absence(self, claim, absence).await
    }

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
