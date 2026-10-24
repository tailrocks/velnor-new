use velnor_runner_github::Method;

use super::super::support::{
    ADMIN_TOKEN, ContextFixture, RecoveryFixture, Reply, Script, recovery_fixture, run_reconcile,
    runner_absence,
};
use crate::error::HostError;
use crate::launch::jit_conflict::ReconcileContext;

#[tokio::test]
async fn initial_empty_lookup_retries_recorded_absence_without_delete() -> Result<(), HostError> {
    Box::pin(initial_empty_lookup_retries_recorded_absence_without_delete_inner()).await
}

async fn initial_empty_lookup_retries_recorded_absence_without_delete_inner()
-> Result<(), HostError> {
    let fixture = recovery_fixture('c', 843, 1_143).await?;
    let mut inputs = ContextFixture::new(&fixture)?;
    let context = inputs.context(&fixture);
    assert_initial_observation_is_unrecorded(&fixture).await?;
    assert_first_empty_lookup_persists_absence(&fixture, &context).await?;
    assert_recorded_absence_confirms_without_delete(&fixture, &context).await?;
    inputs.finish_without_docker_requests().await?;
    Ok(())
}

async fn assert_initial_observation_is_unrecorded(
    fixture: &RecoveryFixture,
) -> Result<(), HostError> {
    assert!(
        fixture
            .scope
            .jit_conflict_current_observation(&fixture.claim)
            .await?
            .is_none()
    );
    Ok(())
}

async fn assert_first_empty_lookup_persists_absence(
    fixture: &RecoveryFixture,
    context: &ReconcileContext<'_>,
) -> Result<(), HostError> {
    let mut first_identity = Script::repository_scope(&fixture.binding, 1);
    let mut first_admin = Script::new(
        vec![
            runner_absence(&fixture.requested_name),
            Reply::new(
                Method::Get,
                "_apis/distributedtask/pools/0/agents",
                Some(format!(
                    "agentName={}&api-version=6.0-preview",
                    fixture.requested_name
                )),
                503,
                "{}",
            ),
        ],
        ADMIN_TOKEN,
    );
    assert!(
        !run_reconcile(
            context,
            &fixture.claim,
            &mut first_identity,
            &mut first_admin
        )
        .await?
    );
    assert_eq!(first_admin.seen.len(), 2);
    assert!(
        first_admin
            .seen
            .iter()
            .all(|request| request.method == Method::Get)
    );
    assert!(first_admin.expected.is_empty());
    assert_eq!(first_identity.seen.len(), 1);
    assert!(first_identity.expected.is_empty());
    assert!(
        fixture
            .scope
            .jit_conflict_current_observation(&fixture.claim)
            .await?
            .is_some_and(|target| target.is_none())
    );
    assert_eq!(empty_progress_state(fixture).await?, (1, 0, 0));
    Ok(())
}

async fn assert_recorded_absence_confirms_without_delete(
    fixture: &RecoveryFixture,
    context: &ReconcileContext<'_>,
) -> Result<(), HostError> {
    let mut retry_identity = Script::repository_scope(&fixture.binding, 1);
    let mut retry_admin = Script::new(
        vec![
            runner_absence(&fixture.requested_name),
            runner_absence(&fixture.requested_name),
        ],
        ADMIN_TOKEN,
    );
    assert!(
        run_reconcile(
            context,
            &fixture.claim,
            &mut retry_identity,
            &mut retry_admin
        )
        .await?
    );
    assert_eq!(retry_admin.seen.len(), 2);
    assert!(
        retry_admin
            .seen
            .iter()
            .all(|request| request.method == Method::Get)
    );
    assert!(retry_admin.expected.is_empty());
    assert_eq!(retry_identity.seen.len(), 1);
    assert!(retry_identity.expected.is_empty());
    assert!(
        fixture
            .scope
            .jit_conflict_current_observation(&fixture.claim)
            .await?
            .is_some_and(|target| target.is_none())
    );
    assert_eq!(empty_progress_state(fixture).await?, (1, 1, 1));
    assert!(
        !fixture
            .scope
            .jit_conflict_delete_started(&fixture.claim)
            .await?
    );
    Ok(())
}

async fn empty_progress_state(fixture: &RecoveryFixture) -> Result<(i64, i64, i64), HostError> {
    let connection = fixture.journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT (SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=2 AND phase='remote_initial_absent' AND runner_id IS NULL AND runner_name=?3 AND runner_scale_set_id=?4 AND lookup_status=200 AND lookup_count=0 AND request_sha256 IS NOT NULL AND response_sha256 IS NOT NULL AND evidence_sha256 IS NOT NULL),(SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=4 AND phase='remote_absent' AND runner_id IS NULL AND runner_name=?3 AND runner_scale_set_id=?4 AND lookup_status=200 AND lookup_count=0 AND request_sha256 IS NOT NULL AND response_sha256 IS NOT NULL AND evidence_sha256 IS NOT NULL),(SELECT remote_cleanup_proven FROM capacity_reservations WHERE id=?1)",
            (
                fixture.claim.reservation_id(),
                fixture.claim.claim_generation(),
                fixture.requested_name.as_str(),
                fixture.claim.scale_set_id(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok((
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
    ))
}
