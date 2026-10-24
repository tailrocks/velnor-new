//! Source-backed recovery journal API tests.

use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers::{
    Scratch, binding, created_scope, provision_acquired_request_with_initialization,
};
use crate::journal::{CapacityLimits, Journal};

const SET_ID: i64 = 824;
const REQUEST_ID: i64 = 1_124;

#[tokio::test]
async fn conflict_candidate_is_claimed_once_and_bound_to_its_scope() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("jit-conflict-recovery-claim", 101)?;
    let scope = created_scope(&journal, &repository, SET_ID).await?;
    let physical =
        provision_acquired_request_with_initialization(&scope, REQUEST_ID, 'e', 'e').await?;
    let requested_name = format!("v{}", "e".repeat(32));
    let (authority_generation, authority_digest) = scope.test_active_authority_identity().await?;
    let jit_claim = scope
        .claim_runner_jit_for_request(
            physical.intent_id,
            physical.reservation_id,
            REQUEST_ID,
            &requested_name,
            authority_generation,
            &authority_digest,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .mark_runner_jit_conflict_received(
            jit_claim,
            physical.reservation_id,
            authority_generation,
            &authority_digest,
        )
        .await?;

    let candidates = scope.recoverable_jit_conflict_candidates().await?;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].reservation_id(), physical.reservation_id);
    let recovery_claim = scope
        .claim_jit_conflict_candidate(&candidates[0], 180)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(recovery_claim.claim_generation(), 1);
    assert_eq!(recovery_claim.requested_name(), requested_name);
    assert_eq!(recovery_claim.scale_set_id(), SET_ID);
    scope.validate_jit_conflict_claim(&recovery_claim).await?;
    assert!(
        scope
            .claim_jit_conflict_candidate(&candidates[0], 180)
            .await?
            .is_none()
    );

    let foreign_binding = binding("jit-conflict-foreign-scope", 102)?;
    let foreign_scope = created_scope(&journal, &foreign_binding, SET_ID + 1).await?;
    assert!(
        foreign_scope
            .validate_jit_conflict_claim(&recovery_claim)
            .await
            .is_err()
    );
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}
