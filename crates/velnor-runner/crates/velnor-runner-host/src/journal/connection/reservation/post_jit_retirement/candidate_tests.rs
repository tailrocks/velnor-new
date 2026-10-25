use crate::error::HostError;
use crate::journal::request_lifecycle::tests::{helpers, native_started_intake_fixture};

pub(super) async fn recover_interrupted_fixture(
    journal: &crate::journal::Journal,
    path: &std::path::Path,
) -> Result<(), HostError> {
    use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};

    let canonical_path = canonical_journal_path(path)?;
    let state_dir = path.parent().ok_or(HostError::Path)?;
    let daemon_lock_path = state_dir.join("daemon.lock");
    let daemon_lock = DaemonLock::try_acquire(&daemon_lock_path)?;
    let engine_id = journal.engine_id().await?;
    let instance_id = journal.instance_id().await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(&engine_id, state_dir)?;
    guard.verify_lineage(&canonical_path, &instance_id, journal.revision().await?)?;
    journal.attach_lineage_guard(guard.clone())?;
    journal.bootstrap_header_authority().await?;
    let authority = crate::launch::SessionRefreshStartupAuthority::test_only(
        &daemon_lock,
        &daemon_lock_path,
        &canonical_path,
        &instance_id,
        &engine_id,
        journal.revision().await?,
        &guard,
    )?;
    journal.recover_interrupted_host_work(authority).await
}

#[tokio::test]
async fn candidate_enumeration_is_scoped_and_skips_healthy_or_active_rows() -> Result<(), HostError>
{
    let fixture = native_started_intake_fixture().await?;
    assert!(
        fixture
            .scope
            .recoverable_post_jit_retirements()
            .await?
            .is_empty()
    );

    recover_interrupted_fixture(&fixture.journal, &fixture.database_path).await?;
    let candidates = fixture.scope.recoverable_post_jit_retirements().await?;
    assert_eq!(candidates.len(), 1);
    let candidate = candidates.into_iter().next().ok_or(HostError::Journal)?;
    assert_eq!(candidate.reservation_id(), fixture.reservation_id);
    assert_eq!(candidate.generation(), 1);
    assert_eq!(candidate.intent_id(), fixture.intent_id);

    let other_binding =
        helpers::binding("retirement-candidate-foreign", 402).map_err(|_| HostError::Identity)?;
    let other_scope = helpers::created_scope(
        &fixture.journal,
        &other_binding,
        fixture.channel.scale_set_id() + 1,
    )
    .await?;
    assert!(
        other_scope
            .recoverable_post_jit_retirements()
            .await?
            .is_empty()
    );

    let claim = fixture
        .scope
        .claim_recoverable_post_jit_retirement(&candidate, 60)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(claim.reservation_id(), fixture.reservation_id);
    assert!(
        fixture
            .scope
            .recoverable_post_jit_retirements()
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn expired_claim_candidate_retries_same_attempt_in_next_generation() -> Result<(), HostError>
{
    let fixture = native_started_intake_fixture().await?;
    recover_interrupted_fixture(&fixture.journal, &fixture.database_path).await?;
    let first_candidate = fixture
        .scope
        .recoverable_post_jit_retirements()
        .await?
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    let first = fixture
        .scope
        .claim_recoverable_post_jit_retirement(&first_candidate, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .recoverable_post_jit_retirements()
            .await?
            .is_empty()
    );

    tokio::time::sleep(std::time::Duration::from_secs(11)).await;
    let retry_candidate = fixture
        .scope
        .recoverable_post_jit_retirements()
        .await?
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    assert_eq!(retry_candidate.reservation_id(), first.reservation_id());
    assert_eq!(retry_candidate.generation(), first.generation());
    let retry = fixture
        .scope
        .claim_recoverable_post_jit_retirement(&retry_candidate, 60)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(retry.claim_generation(), first.claim_generation() + 1);
    assert_eq!(retry.reservation_id(), first.reservation_id());
    assert_eq!(retry.generation(), first.generation());
    Ok(())
}
