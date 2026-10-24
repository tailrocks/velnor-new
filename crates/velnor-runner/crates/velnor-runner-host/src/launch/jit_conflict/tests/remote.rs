use velnor_runner_github::Method;

use super::support::{
    ADMIN_TOKEN, ContextFixture, RecoveryFixture, Reply, Script, recovery_fixture, run_reconcile,
    runner_absence, runner_lookup, scalar_pair,
};
use crate::error::HostError;
use crate::launch::jit_conflict::ReconcileContext;

#[tokio::test]
async fn accepted_delete_then_absence_records_one_target_and_one_delete() -> Result<(), HostError> {
    Box::pin(accepted_delete_then_absence_records_one_target_and_one_delete_inner()).await
}

async fn accepted_delete_then_absence_records_one_target_and_one_delete_inner()
-> Result<(), HostError> {
    let fixture = recovery_fixture('a', 841, 1_141).await?;
    let mut inputs = ContextFixture::new(&fixture)?;
    let context = inputs.context(&fixture);
    let mut identity = Script::repository_scope(&fixture.binding, 4);
    let mut admin = Script::new(
        vec![
            runner_lookup(&fixture.requested_name, fixture.claim.scale_set_id(), 71),
            Reply::new(
                Method::Delete,
                "_apis/distributedtask/pools/0/agents/71",
                Some("api-version=6.0-preview".to_owned()),
                204,
                "",
            ),
            runner_absence(&fixture.requested_name),
        ],
        ADMIN_TOKEN,
    );

    assert!(run_reconcile(&context, &fixture.claim, &mut identity, &mut admin).await?);
    assert_eq!(admin.seen.len(), 3);
    assert_eq!(identity.seen.len(), 4);
    assert_eq!(admin.seen[0].method, Method::Get);
    assert_eq!(admin.seen[0].path, "_apis/distributedtask/pools/0/agents");
    assert_eq!(admin.seen[1].method, Method::Delete);
    assert_eq!(
        admin.seen[1].path,
        "_apis/distributedtask/pools/0/agents/71"
    );
    assert!(
        identity
            .seen
            .iter()
            .all(|request| request.path.trim_start_matches('/') == "repos/owner-101/repo-101")
    );
    assert!(admin.expected.is_empty());
    assert!(identity.expected.is_empty());
    assert!(
        fixture
            .scope
            .jit_conflict_delete_started(&fixture.claim)
            .await?
    );
    assert_eq!(
        fixture
            .scope
            .jit_conflict_delete_target(&fixture.claim)
            .await?
            .map(|target| target.id),
        Some(71)
    );
    assert_eq!(
        fixture
            .scope
            .jit_conflict_current_observation(&fixture.claim)
            .await?
            .map(|target| target.map(|target| target.id)),
        Some(Some(71))
    );
    assert_final_absence_record(&fixture, 71).await?;
    inputs.finish_without_docker_requests().await?;
    Ok(())
}

async fn assert_final_absence_record(
    fixture: &RecoveryFixture,
    runner_id: i64,
) -> Result<(), HostError> {
    let mut rows = fixture
        .journal
        .connection()
        .await?
        .query(
            "SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=4 AND phase='remote_absent' AND runner_id=?3 AND runner_name=?4 AND runner_scale_set_id=?5 AND lookup_status=200 AND lookup_count=0 AND request_sha256 IS NOT NULL AND response_sha256 IS NOT NULL AND evidence_sha256 IS NOT NULL",
            (
                fixture.claim.reservation_id(),
                fixture.claim.claim_generation(),
                runner_id,
                fixture.requested_name.as_str(),
                fixture.claim.scale_set_id(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(count, 1);
    Ok(())
}

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

#[tokio::test]
async fn rejected_busy_delete_holds_target_and_recovery_never_replays_delete()
-> Result<(), HostError> {
    Box::pin(rejected_busy_delete_holds_target_and_recovery_never_replays_delete_inner()).await
}

async fn rejected_busy_delete_holds_target_and_recovery_never_replays_delete_inner()
-> Result<(), HostError> {
    let fixture = recovery_fixture('b', 842, 1_142).await?;
    let mut inputs = ContextFixture::new(&fixture)?;
    let context = inputs.context(&fixture);
    assert_busy_delete_stops_before_settlement(&fixture, &context).await?;
    assert_retry_with_target_never_deletes(&fixture, &context).await?;
    assert_absence_retry_never_deletes(&fixture, &context).await?;
    inputs.finish_without_docker_requests().await?;
    Ok(())
}

async fn assert_busy_delete_stops_before_settlement(
    fixture: &RecoveryFixture,
    context: &ReconcileContext<'_>,
) -> Result<(), HostError> {
    let mut identity = Script::repository_scope(&fixture.binding, 3);
    let mut admin = Script::new(
        vec![
            runner_lookup(&fixture.requested_name, fixture.claim.scale_set_id(), 72),
            Reply::new(
                Method::Delete,
                "_apis/distributedtask/pools/0/agents/72",
                Some("api-version=6.0-preview".to_owned()),
                409,
                "{}",
            ),
        ],
        ADMIN_TOKEN,
    );

    assert!(!run_reconcile(context, &fixture.claim, &mut identity, &mut admin).await?);
    assert!(admin.expected.is_empty());
    assert!(identity.expected.is_empty());
    assert_eq!(
        admin
            .seen
            .iter()
            .filter(|request| request.method == Method::Delete)
            .count(),
        1
    );
    assert!(
        fixture
            .scope
            .jit_conflict_delete_started(&fixture.claim)
            .await?
    );
    assert_eq!(
        fixture
            .scope
            .jit_conflict_current_observation(&fixture.claim)
            .await?
            .map(|target| target.map(|target| target.id)),
        Some(Some(72))
    );
    assert_eq!(
        scalar_pair(
            &fixture.journal.connection().await?,
            fixture.claim.reservation_id()
        )
        .await?,
        (1, 0)
    );
    Ok(())
}

async fn assert_retry_with_target_never_deletes(
    fixture: &RecoveryFixture,
    context: &ReconcileContext<'_>,
) -> Result<(), HostError> {
    let mut retry_identity = Script::repository_scope(&fixture.binding, 0);
    let mut retry_admin = Script::new(
        vec![runner_lookup(
            &fixture.requested_name,
            fixture.claim.scale_set_id(),
            72,
        )],
        ADMIN_TOKEN,
    );
    assert!(
        !run_reconcile(
            context,
            &fixture.claim,
            &mut retry_identity,
            &mut retry_admin,
        )
        .await?
    );
    assert_eq!(retry_admin.seen.len(), 1);
    assert!(
        retry_admin
            .seen
            .iter()
            .all(|request| request.method != Method::Delete)
    );
    assert!(retry_admin.expected.is_empty());
    assert!(retry_identity.expected.is_empty());
    assert_eq!(
        scalar_pair(
            &fixture.journal.connection().await?,
            fixture.claim.reservation_id()
        )
        .await?,
        (1, 0)
    );
    Ok(())
}

async fn assert_absence_retry_never_deletes(
    fixture: &RecoveryFixture,
    context: &ReconcileContext<'_>,
) -> Result<(), HostError> {
    let mut absence_identity = Script::repository_scope(&fixture.binding, 1);
    let mut absence_admin = Script::new(vec![runner_absence(&fixture.requested_name)], ADMIN_TOKEN);
    assert!(
        run_reconcile(
            context,
            &fixture.claim,
            &mut absence_identity,
            &mut absence_admin,
        )
        .await?
    );
    assert!(absence_admin.expected.is_empty());
    assert!(absence_identity.expected.is_empty());
    assert!(
        absence_admin
            .seen
            .iter()
            .all(|request| request.method != Method::Delete)
    );
    assert_eq!(absence_admin.seen.len(), 1);
    assert_eq!(
        scalar_pair(
            &fixture.journal.connection().await?,
            fixture.claim.reservation_id()
        )
        .await?,
        (1, 0)
    );
    Ok(())
}
