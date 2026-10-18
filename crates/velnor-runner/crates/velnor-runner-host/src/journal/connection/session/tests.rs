//! Scoped session and host-wide header authority regressions.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::{CapacityLimits, ConnectionJournal, Journal, SessionCreateState};
use crate::launch::{CompletedHeaderPoll, DeletedSession204};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-scoped-session-{}-{sequence}",
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
    max_jobs: u32,
) -> Result<(Journal, ConnectionJournal), HostError> {
    let journal = Journal::open(path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: max_jobs,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await?;
    journal.bootstrap_header_authority().await?;
    journal.bind_engine("test-engine").await?;
    let binding = binding(id, repository_id)?;
    let scope = journal.connection_scope(&binding).await?;
    scope
        .bind_scale_set(repository_id + 10_000, "integration", SetOwnership::Adopted)
        .await?;
    Ok((journal, scope))
}

#[tokio::test]
async fn returned_session_and_header_commit_together_and_replay_after_restart()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let (journal, scope) = open_scope(&path, "essential", 101, 2).await?;
    let prepared = scope
        .prepare_session_create("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .await?;
    assert_eq!(prepared.state(), SessionCreateState::Prepared);
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let bound = scope
        .record_created_session(&claim, "remote-session-a", 2)
        .await?;
    assert_eq!(bound.allocation().target(), 2);
    assert_eq!(journal.charged_header_capacity().await?, 2);
    assert!(!scope.is_drained().await?);
    drop(scope);
    drop(journal);

    let (journal, scope) = open_scope(&path, "essential", 101, 2).await?;
    let replay = scope.session_creates().await?;
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].state(), SessionCreateState::Bound);
    assert_eq!(replay[0].session_id(), Some("remote-session-a"));
    assert_eq!(replay[0].intent_id(), Some(bound.intent_id()));
    assert_eq!(journal.header_allocations().await?.len(), 1);
    assert_eq!(journal.charged_header_capacity().await?, 2);
    Ok(())
}

#[tokio::test]
async fn connection_sessions_share_one_limit_and_overbudget_response_binds_zero()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let (journal, first) = open_scope(&scratch.file(), "essential", 101, 2).await?;
    let second = journal
        .connection_scope(&binding("chainargos", 202)?)
        .await?;
    second
        .bind_scale_set(10_202, "integration", SetOwnership::Adopted)
        .await?;

    let first_create = first
        .prepare_session_create("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .await?;
    let first_claim = first
        .claim_session_create(first_create.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let first_bound = first
        .record_created_session(&first_claim, "same-session", 2)
        .await?;
    assert_eq!(first_bound.allocation().target(), 2);

    let second_create = second
        .prepare_session_create("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .await?;
    let second_claim = second
        .claim_session_create(second_create.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let second_bound = second
        .record_created_session(&second_claim, "same-session", 1)
        .await?;
    assert_eq!(second_bound.allocation().target(), 0);
    assert_eq!(journal.charged_header_capacity().await?, 2);
    assert_eq!(journal.header_allocations().await?.len(), 2);
    assert!(!journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn uncertain_create_survives_restart_and_blocks_a_second_session() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let (journal, scope) = open_scope(&path, "essential", 101, 1).await?;
    let prepared = scope
        .prepare_session_create("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        .await?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    scope.mark_session_create_uncertain(&claim).await?;
    assert!(
        scope
            .claim_session_create(prepared.local_nonce())
            .await?
            .is_none()
    );
    assert!(
        scope
            .prepare_session_create("cccccccccccccccccccccccccccccccc")
            .await
            .is_err()
    );
    assert!(!journal.is_drained().await?);
    drop(scope);
    drop(journal);

    let (journal, scope) = open_scope(&path, "essential", 101, 1).await?;
    assert_eq!(
        scope.session_creates().await?[0].state(),
        SessionCreateState::Uncertain
    );
    assert!(
        scope
            .claim_session_create("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
            .await?
            .is_none()
    );
    assert!(
        scope
            .prepare_session_create("cccccccccccccccccccccccccccccccc")
            .await
            .is_err()
    );
    assert!(!journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn exact_zero_poll_and_delete_retire_header_and_session_atomically() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let (journal, scope) = open_scope(&scratch.file(), "essential", 101, 1).await?;
    let prepared = scope
        .prepare_session_create("dddddddddddddddddddddddddddddddd")
        .await?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let bound = scope
        .record_created_session(&claim, "remote-session", 1)
        .await?;
    let intent_id = bound.intent_id();
    let allocation = bound.allocation().clone();

    let poll = scope.begin_session_poll(intent_id, &allocation).await?;
    scope
        .settle_session_poll(intent_id, CompletedHeaderPoll::test_only(poll))
        .await?;
    let zero = scope.begin_zero_header_poll(intent_id, &allocation).await?;
    assert_eq!(zero.header_value(), 0);
    scope
        .settle_session_poll(intent_id, CompletedHeaderPoll::test_only(zero))
        .await?;
    let delete = scope.begin_session_delete(intent_id, &allocation).await?;
    scope
        .retire_session_after_delete(intent_id, DeletedSession204::test_only(delete))
        .await?;

    assert_eq!(journal.charged_header_capacity().await?, 0);
    assert_eq!(
        scope.session_creates().await?[0].state(),
        SessionCreateState::Retired
    );
    assert!(scope.is_drained().await?);
    assert!(journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn interrupted_poll_stays_charged_and_cannot_be_retired() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let (journal, scope) = open_scope(&path, "essential", 101, 1).await?;
    let prepared = scope
        .prepare_session_create("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")
        .await?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let bound = scope
        .record_created_session(&claim, "remote-session", 1)
        .await?;
    let allocation = bound.allocation().clone();
    let intent_id = bound.intent_id();
    let _in_flight = scope.begin_session_poll(intent_id, &allocation).await?;
    drop(scope);
    drop(journal);

    let (journal, scope) = open_scope(&path, "essential", 101, 1).await?;
    assert_eq!(journal.charged_header_capacity().await?, 1);
    assert!(
        scope
            .begin_session_poll(intent_id, &allocation)
            .await
            .is_err()
    );
    assert!(
        scope
            .begin_session_delete(intent_id, &allocation)
            .await
            .is_err()
    );
    assert!(!scope.is_drained().await?);
    assert!(!journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn altered_session_index_predicate_fails_closed_on_reopen() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let connection = journal.connection().await?;
    connection
        .execute("DROP INDEX connection_session_create_active", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE UNIQUE INDEX connection_session_create_active ON connection_session_creates(connection_id, scale_set_id) WHERE state IN ('prepared', 'requested', 'uncertain')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(journal);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn altered_session_table_ddl_fails_closed_on_reopen() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let connection = journal.connection().await?;
    connection
        .execute(
            "ALTER TABLE connection_session_creates ADD COLUMN unexpected TEXT",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(journal);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}
