//! Journal authority and progress for orphaned JIT-name conflicts.

mod claim;
pub(in crate::journal) mod identity;
mod progress;
mod settlement;

#[cfg(test)]
#[path = "jit_conflict/tests.rs"]
mod tests;

pub(crate) use claim::{JitConflictRecoveryCandidate, JitConflictRecoveryClaim};

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

impl ConnectionJournal {
    pub(crate) async fn recoverable_jit_conflict_candidates(
        &self,
    ) -> Result<Vec<JitConflictRecoveryCandidate>, HostError> {
        claim::recoverable_candidates(self).await
    }

    pub(crate) async fn claim_jit_conflict_candidate(
        &self,
        candidate: &JitConflictRecoveryCandidate,
        lease_seconds: i64,
    ) -> Result<Option<JitConflictRecoveryClaim>, HostError> {
        claim::claim_candidate(self, candidate, lease_seconds).await
    }

    pub(crate) async fn validate_jit_conflict_claim(
        &self,
        claim: &JitConflictRecoveryClaim,
    ) -> Result<(), HostError> {
        progress::validate_claim(self, claim).await
    }

    pub(crate) async fn record_jit_conflict_local_absence(
        &self,
        claim: &JitConflictRecoveryClaim,
        proof: crate::launch::bootstrap::VerifiedStorageCleanup,
    ) -> Result<(), HostError> {
        progress::record_local_absence(self, claim, proof).await
    }

    pub(crate) async fn resume_jit_conflict_local_absence(
        &self,
        claim: &JitConflictRecoveryClaim,
    ) -> Result<bool, HostError> {
        progress::resume_local_absence(self, claim).await
    }

    pub(crate) async fn record_jit_conflict_remote_target(
        &self,
        claim: &JitConflictRecoveryClaim,
        runner: &velnor_runner_github::RunnerReference,
        receipt: &velnor_runner_github::RunnerResponseReceipt,
    ) -> Result<(), HostError> {
        progress::record_remote_target(self, claim, runner, receipt).await
    }

    pub(crate) async fn record_jit_conflict_remote_initial_absence(
        &self,
        claim: &JitConflictRecoveryClaim,
        receipt: &velnor_runner_github::RunnerResponseReceipt,
    ) -> Result<(), HostError> {
        progress::record_remote_initial_absence(self, claim, receipt).await
    }

    pub(crate) async fn jit_conflict_delete_started(
        &self,
        claim: &JitConflictRecoveryClaim,
    ) -> Result<bool, HostError> {
        claim::has_delete_started(self, claim).await
    }

    pub(crate) async fn jit_conflict_delete_target(
        &self,
        claim: &JitConflictRecoveryClaim,
    ) -> Result<Option<velnor_runner_github::RunnerReference>, HostError> {
        claim::delete_target(self, claim).await
    }

    pub(crate) async fn jit_conflict_current_observation(
        &self,
        claim: &JitConflictRecoveryClaim,
    ) -> Result<Option<Option<velnor_runner_github::RunnerReference>>, HostError> {
        claim::current_observation(self, claim).await
    }

    pub(crate) async fn record_jit_conflict_delete_started(
        &self,
        claim: &JitConflictRecoveryClaim,
        runner: &velnor_runner_github::RunnerReference,
    ) -> Result<(), HostError> {
        progress::record_delete_started(self, claim, runner).await
    }

    pub(crate) async fn record_jit_conflict_remote_absence(
        &self,
        claim: &JitConflictRecoveryClaim,
        receipt: &velnor_runner_github::RunnerResponseReceipt,
    ) -> Result<(), HostError> {
        progress::record_remote_absence(self, claim, receipt).await
    }
}

pub(in crate::journal::connection::reservation) async fn record_conflict_metadata_settlement(
    connection: &turso::Connection,
    reservation_id: i64,
    metadata_generation: i64,
) -> Result<bool, HostError> {
    settlement::settle_after_metadata(connection, reservation_id, metadata_generation).await
}

pub(in crate::journal::connection::reservation) async fn has_conflict_recovery_claim(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    settlement::has_claim(connection, reservation_id).await
}

pub(in crate::journal::connection::reservation) async fn conflict_recovery_metadata_claim_ready(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    settlement::metadata_claim_ready(connection, reservation_id).await
}

pub(super) async fn validate_conflict_settlement_digests(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    settlement::validate_persisted_settlements(connection).await
}
