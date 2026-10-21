use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
use crate::error::HostError;
use crate::journal::{Journal, request_lifecycle::tests::NativeStartedIntakeFixture};

pub(crate) async fn recover_fixture(fixture: &NativeStartedIntakeFixture) -> Result<(), HostError> {
    let canonical_path = canonical_journal_path(&fixture.database_path)?;
    let state_dir = fixture.database_path.parent().ok_or(HostError::Path)?;
    let daemon_lock_path = state_dir.join("daemon.lock");
    let daemon_lock = DaemonLock::try_acquire(&daemon_lock_path)?;
    let engine_id = fixture.journal.engine_id().await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(&engine_id, state_dir)?;
    let instance_id = fixture.journal.instance_id().await?;
    let revision = fixture.journal.revision().await?;
    guard.verify_lineage(&canonical_path, &instance_id, revision)?;
    fixture.journal.attach_lineage_guard(guard.clone())?;
    fixture.journal.bootstrap_header_authority().await?;
    let instance_id = fixture.journal.instance_id().await?;
    let revision = fixture.journal.revision().await?;
    let authority = crate::launch::SessionRefreshStartupAuthority::test_only(
        &daemon_lock,
        &daemon_lock_path,
        &canonical_path,
        &instance_id,
        &engine_id,
        revision,
        &guard,
    )?;
    fixture
        .journal
        .recover_interrupted_host_work(authority)
        .await
}

pub(crate) async fn reopen_and_recover_fixture(
    fixture: &NativeStartedIntakeFixture,
) -> Result<Journal, HostError> {
    let journal = Journal::open(&fixture.database_path).await?;
    let canonical_path = canonical_journal_path(&fixture.database_path)?;
    let state_dir = fixture.database_path.parent().ok_or(HostError::Path)?;
    let daemon_lock_path = state_dir.join("daemon.lock");
    let daemon_lock = DaemonLock::try_acquire(&daemon_lock_path)?;
    let engine_id = journal.engine_id().await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(&engine_id, state_dir)?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    guard.verify_lineage(&canonical_path, &instance_id, revision)?;
    journal.attach_lineage_guard(guard.clone())?;
    journal.bootstrap_header_authority().await?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    let authority = crate::launch::SessionRefreshStartupAuthority::test_only(
        &daemon_lock,
        &daemon_lock_path,
        &canonical_path,
        &instance_id,
        &engine_id,
        revision,
        &guard,
    )?;
    journal.recover_interrupted_host_work(authority).await?;
    Ok(journal)
}
