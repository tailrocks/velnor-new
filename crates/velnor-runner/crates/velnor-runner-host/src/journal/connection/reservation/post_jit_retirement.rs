//! This module records retirement claims after JIT configuration delivery.

mod claim;
#[cfg(test)]
mod claim_tests;
mod disposition;
#[cfg(test)]
mod effect_fence_tests;
mod identity;

pub(crate) use claim::PostJitRetirementClaim;
pub(in crate::journal) use disposition::EventRetirementDisposition;

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

impl ConnectionJournal {
    /// Create a retirement claim for one physical attempt with a specified time limit.
    pub(crate) async fn claim_post_jit_retirement(
        &self,
        reservation_id: i64,
        lease_seconds: i64,
    ) -> Result<Option<PostJitRetirementClaim>, HostError> {
        claim::claim(self, reservation_id, lease_seconds).await
    }
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
