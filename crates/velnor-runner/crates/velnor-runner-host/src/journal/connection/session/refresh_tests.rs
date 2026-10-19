//! Refresh claims retain their scope, history, and unrelated header uncertainty.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::connect::SetOwnership;
use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
use crate::error::HostError;
use crate::journal::{
    CapacityLimits, ConnectionJournal, Journal, SessionCreateState, SessionRefreshState,
};
use crate::launch::{ReturnedSessionRefreshTransportError, VerifiedSessionRefresh};

#[path = "refresh_tests/v13_fixture.rs"]
mod v13_fixture;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-session-refresh-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

fn binding(id: &str, repository_id: i64) -> Result<RepositoryConnection, HostError> {
    let connection_id = ConnectionId::parse(id)?;
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::BranchRestricted,
        vec!["main".to_owned()],
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )?;
    RepositoryConnection::try_new(
        connection_id,
        repository_id,
        format!("owner/{id}"),
        KeychainReference::parse(&format!("keychain:velnor/{id}"))?,
        selector,
        policy,
    )
}

async fn open_scope(
    path: &Path,
    id: &str,
    repository_id: i64,
) -> Result<(Journal, ConnectionJournal), HostError> {
    let journal = Journal::open(path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: 2_000,
            memory_bytes: 2_147_483_648,
        })
        .await?;
    journal.bootstrap_header_authority().await?;
    journal.bind_engine("test-engine").await?;
    let scope = journal
        .connection_scope(&binding(id, repository_id)?)
        .await?;
    scope
        .bind_scale_set(repository_id + 10_000, "integration", SetOwnership::Adopted)
        .await?;
    Ok((journal, scope))
}

async fn bind_session(
    scope: &ConnectionJournal,
    nonce: &str,
    session_id: &str,
) -> Result<(i64, crate::journal::HeaderAllocation), HostError> {
    let prepared = scope.prepare_session_create(nonce).await?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let bound = scope.record_created_session(&claim, session_id, 1).await?;
    Ok((bound.intent_id(), bound.allocation().clone()))
}

#[tokio::test]
async fn successful_generations_are_exact_and_mismatched_response_stays_requested()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let (_journal, scope) = open_scope(&scratch.file(), "essential", 101).await?;
    let (intent_id, _allocation) = bind_session(
        &scope,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "remote-session-a",
    )
    .await?;

    let first = scope
        .claim_session_refresh(intent_id, "remote-session-a")
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(first.generation(), 1);
    assert!(
        scope
            .claim_session_refresh(intent_id, "remote-session-a")
            .await?
            .is_none()
    );
    let first_record = scope
        .record_session_refreshed(
            first,
            VerifiedSessionRefresh::test_only(intent_id, "remote-session-a", 1),
        )
        .await?;
    assert_eq!(first_record.state(), SessionRefreshState::Succeeded);

    let second = scope
        .claim_session_refresh(intent_id, "remote-session-a")
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(second.generation(), 2);
    assert!(
        scope
            .record_session_refreshed(
                second,
                VerifiedSessionRefresh::test_only(intent_id, "remote-session-b", 2),
            )
            .await
            .is_err()
    );
    let history = scope.session_refreshes(intent_id).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].state(), SessionRefreshState::Succeeded);
    assert_eq!(history[1].state(), SessionRefreshState::Requested);
    assert_eq!(history[1].session_id(), "remote-session-a");
    Ok(())
}

#[tokio::test]
async fn returned_transport_error_allows_a_new_generation_without_settling_poll()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let (journal, scope) = open_scope(&path, "essential", 101).await?;
    let (intent_id, allocation) = bind_session(
        &scope,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "remote-session-b",
    )
    .await?;
    let poll = scope.begin_session_poll(intent_id, &allocation).await?;
    scope.mark_session_poll_uncertain(intent_id, poll).await?;

    let first = scope
        .claim_session_refresh(intent_id, "remote-session-b")
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .record_session_refresh_transport_error(
            first,
            ReturnedSessionRefreshTransportError::test_only(intent_id, "remote-session-b", 1),
        )
        .await?;
    assert_eq!(journal.charged_header_capacity().await?, 1);
    drop(scope);
    drop(journal);

    let (journal, scope) = open_scope(&path, "essential", 101).await?;
    let second = scope
        .claim_session_refresh(intent_id, "remote-session-b")
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(second.generation(), 2);
    scope
        .record_session_refreshed(
            second,
            VerifiedSessionRefresh::test_only(intent_id, "remote-session-b", 2),
        )
        .await?;
    let history = scope.session_refreshes(intent_id).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].state(), SessionRefreshState::TransportError);
    assert_eq!(history[1].state(), SessionRefreshState::Succeeded);
    assert_eq!(journal.charged_header_capacity().await?, 1);
    assert!(
        scope
            .begin_session_poll(intent_id, &allocation)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn unknown_refresh_result_survives_restart_and_blocks_a_new_generation()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let (journal, scope) = open_scope(&path, "essential", 101).await?;
    let (intent_id, _allocation) = bind_session(
        &scope,
        "cccccccccccccccccccccccccccccccc",
        "remote-session-c",
    )
    .await?;
    let claim = scope
        .claim_session_refresh(intent_id, "remote-session-c")
        .await?
        .ok_or(HostError::Journal)?;
    scope.mark_session_refresh_uncertain(claim).await?;
    drop(scope);
    drop(journal);

    let (_journal, scope) = open_scope(&path, "essential", 101).await?;
    assert!(
        scope
            .claim_session_refresh(intent_id, "remote-session-c")
            .await?
            .is_none()
    );
    let history = scope.session_refreshes(intent_id).await?;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].state(), SessionRefreshState::Uncertain);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn startup_authority_marks_interrupted_refresh_before_new_generation() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let canonical_path = canonical_journal_path(&path)?;
    let engine_id = "test-engine";
    let (journal, scope) = open_scope(&path, "essential", 101).await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(engine_id, &scratch.0)?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    guard.verify_lineage(&canonical_path, &instance_id, revision)?;
    journal.attach_lineage_guard(guard.clone())?;
    let (intent_id, _) = bind_session(
        &scope,
        "cccccccccccccccccccccccccccccccc",
        "interrupted-session",
    )
    .await?;
    let interrupted = scope
        .claim_session_refresh(intent_id, "interrupted-session")
        .await?
        .ok_or(HostError::Journal)?;
    drop(interrupted);
    drop(scope);
    drop(journal);
    drop(guard);

    let daemon_lock_path = scratch.0.join("daemon.lock");
    let daemon_lock = DaemonLock::try_acquire(&daemon_lock_path)?;
    let journal = Journal::open(&path).await?;
    journal.bind_engine(engine_id).await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(engine_id, &scratch.0)?;
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
        engine_id,
        revision,
        &guard,
    )?;
    journal.recover_interrupted_host_work(authority).await?;

    let scope = journal
        .connection_scope(&binding("essential", 101)?)
        .await?;
    let history = scope.session_refreshes(intent_id).await?;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].state(), SessionRefreshState::AbandonedUncertain);
    let next = scope
        .claim_session_refresh(intent_id, "interrupted-session")
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(next.generation(), 2);
    assert_eq!(journal.charged_header_capacity().await?, 1);
    Ok(())
}

#[tokio::test]
async fn same_remote_session_id_has_independent_connection_scoped_generations()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let (journal, first) = open_scope(&scratch.file(), "essential", 101).await?;
    let second = journal
        .connection_scope(&binding("chainargos", 202)?)
        .await?;
    second
        .bind_scale_set(10_202, "integration", SetOwnership::Adopted)
        .await?;
    let (first_intent, _) = bind_session(
        &first,
        "dddddddddddddddddddddddddddddddd",
        "same-session-id",
    )
    .await?;
    let (second_intent, _) = bind_session(
        &second,
        "dddddddddddddddddddddddddddddddd",
        "same-session-id",
    )
    .await?;
    let first_claim = first
        .claim_session_refresh(first_intent, "same-session-id")
        .await?
        .ok_or(HostError::Journal)?;
    let second_claim = second
        .claim_session_refresh(second_intent, "same-session-id")
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(first_claim.generation(), 1);
    assert_eq!(second_claim.generation(), 1);
    assert_eq!(first.session_refreshes(first_intent).await?.len(), 1);
    assert_eq!(second.session_refreshes(second_intent).await?.len(), 1);
    assert_eq!(first_claim.session_id(), second_claim.session_id());
    assert_ne!(first_intent, second_intent);
    Ok(())
}

#[tokio::test]
async fn version_thirteen_migration_keeps_bound_session_and_capacity() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let intent_id = v13_fixture::seed(&path).await?;
    let journal = Journal::open(&path).await?;
    let scope = journal
        .connection_scope(&binding("essential", 101)?)
        .await?;

    assert_eq!(journal.charged_header_capacity().await?, 1);
    let creates = scope.session_creates().await?;
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0].intent_id(), Some(intent_id));
    assert_eq!(creates[0].session_id(), Some(v13_fixture::session_id()));
    assert_eq!(creates[0].state(), SessionCreateState::Bound);
    assert_eq!(scope.session_refreshes(intent_id).await?, Vec::new());
    Ok(())
}
