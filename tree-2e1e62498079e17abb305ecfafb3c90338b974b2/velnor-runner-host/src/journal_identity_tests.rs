//! Durable launch identity and slot reservations.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::journal::LaunchReservation;
use crate::{HostError, IntentState, Journal, Outcome};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-launch-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

async fn open(path: &Path) -> Result<Journal, HostError> {
    Journal::open(path).await
}

async fn create_legacy_database(path: &Path, message_id: i64) -> Result<(), HostError> {
    const LEGACY_ID: i64 = 41;
    let path = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO intents (id, kind, subject, state, docker_id, github_runner_id, cleanup_proven) VALUES (?1, 'launch', ?2, 'uncertain', ?3, ?4, 0)",
            (
                LEGACY_ID,
                format!("m{message_id}r42"),
                "legacy-runner-container",
                "legacy-github-runner",
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn assert_legacy_reservation(
    journal: &Journal,
    expected_subject: &str,
    expected_assignment: Option<&str>,
) -> Result<(), HostError> {
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 41);
    assert_eq!(rows[0].kind, "launch");
    assert_eq!(rows[0].subject, expected_subject);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(
        rows[0].docker_id.as_deref(),
        Some("legacy-runner-container")
    );
    assert_eq!(
        rows[0].github_runner_id.as_deref(),
        Some("legacy-github-runner")
    );
    assert_eq!(rows[0].assignment_key.as_deref(), expected_assignment);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.occupied_launches().await?, 1);
    Ok(())
}

#[tokio::test]
async fn assignment_key_survives_queue_redelivery_and_separates_sets() -> Result<(), HostError> {
    let scratch = Scratch::new("assignment")?;
    let journal = open(&scratch.file()).await?;
    let first = journal.reserve_assignment(7, 42, 100, 2).await?;
    let LaunchReservation::New(first_id) = first else {
        return Err(HostError::Journal);
    };
    assert_eq!(
        journal.reserve_assignment(7, 42, 101, 2).await?,
        LaunchReservation::Existing(first_id)
    );
    assert_ne!(
        journal.reserve_assignment(8, 42, 101, 2).await?,
        LaunchReservation::Existing(first_id)
    );
    assert_eq!(journal.rows().await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn legacy_assignment_is_adopted_only_for_its_original_message() -> Result<(), HostError> {
    let adopted = Scratch::new("legacy-adopt")?;
    create_legacy_database(&adopted.file(), 100).await?;
    let journal = open(&adopted.file()).await?;
    assert_eq!(
        journal.reserve_assignment(7, 42, 100, 2).await?,
        LaunchReservation::Existing(41)
    );
    assert_legacy_reservation(&journal, "m100r42", Some("7:42")).await?;

    let mismatched = Scratch::new("legacy-message-mismatch")?;
    create_legacy_database(&mismatched.file(), 100).await?;
    let journal = open(&mismatched.file()).await?;
    assert_eq!(
        journal.reserve_assignment(7, 42, 101, 2).await,
        Err(HostError::Journal)
    );
    assert_legacy_reservation(&journal, "m100r42", None).await?;
    Ok(())
}

#[tokio::test]
async fn uncertain_launch_reserves_slot_until_cleanup_is_proven() -> Result<(), HostError> {
    let scratch = Scratch::new("reservation")?;
    let journal = open(&scratch.file()).await?;
    let LaunchReservation::New(id) = journal.reserve_assignment(7, 42, 100, 1).await? else {
        return Err(HostError::Journal);
    };
    assert_eq!(journal.occupied_launches().await?, 1);
    journal.finish(id, Outcome::Uncertain).await?;
    assert_eq!(journal.occupied_launches().await?, 1);
    let row = journal.rows().await?.remove(0);
    assert_eq!(row.state, IntentState::Uncertain);
    assert!(row.docker_id.is_none());
    assert!(row.dind_id.is_none());
    journal
        .bind_pair(id, "runner-container", "dind-container")
        .await?;
    assert_eq!(
        journal
            .bind_pair(id, "other-runner", "dind-container")
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(journal.occupied_launches().await?, 1);
    assert!(journal.claim_jit(id).await?);
    journal.bind_github_runner(id, "github-runner-71").await?;
    assert_eq!(
        journal.bind_github_runner(id, "github-runner-72").await,
        Err(HostError::Journal)
    );
    journal.record_cleanup(id).await?;
    assert_eq!(journal.occupied_launches().await?, 0);
    assert_eq!(
        journal.bind_pair(id, "runner-late", "dind-container").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.finish(id, Outcome::DefiniteFailure).await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn atomic_reservations_never_exceed_capacity() -> Result<(), HostError> {
    let scratch = Scratch::new("atomic-capacity")?;
    let journal = open(&scratch.file()).await?;
    assert_eq!(
        journal.reserve_assignment(7, 42, 100, 1).await?,
        LaunchReservation::New(1)
    );
    assert_eq!(
        journal.reserve_launch("scale-session-2", 1).await?,
        LaunchReservation::AtCapacity
    );
    assert_eq!(journal.occupied_launches().await?, 1);
    Ok(())
}

#[tokio::test]
async fn concurrent_launches_reserve_one_capacity_slot_once() -> Result<(), HostError> {
    let scratch = Scratch::new("concurrent-capacity")?;
    let journal = open(&scratch.file()).await?;
    let (first, second) = tokio::join!(
        journal.reserve_assignment(7, 42, 100, 1),
        journal.reserve_assignment(7, 43, 101, 1),
    );
    let outcomes = [first?, second?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(**outcome, LaunchReservation::New(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == LaunchReservation::AtCapacity)
            .count(),
        1
    );
    assert_eq!(journal.occupied_launches().await?, 1);
    Ok(())
}

#[tokio::test]
async fn launch_identity_and_seed_generation_are_immutable() -> Result<(), HostError> {
    let scratch = Scratch::new("identity")?;
    let journal = open(&scratch.file()).await?;
    journal.bind_engine("docker-engine-a").await?;
    let bound_revision = journal.revision().await?;
    journal.bind_engine("docker-engine-a").await?;
    assert_eq!(journal.revision().await?, bound_revision);
    assert_eq!(
        journal.bind_engine("docker-engine-b").await,
        Err(HostError::Journal)
    );
    let id = journal.begin("launch", "assignment").await?;
    let identity = journal.launch_identity(id).await?;
    assert_eq!(identity.intent_id(), id);
    assert_eq!(identity.engine_id(), "docker-engine-a");
    assert_eq!(
        identity.private_volume(),
        format!("v{}", identity.launch_id())
    );
    journal
        .bind_seed_generation(id, "generation-sha256-a")
        .await?;
    assert_eq!(
        journal
            .bind_seed_generation(id, "generation-sha256-b")
            .await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn revision_advances_with_committed_launch_changes() -> Result<(), HostError> {
    let scratch = Scratch::new("revision")?;
    let journal = open(&scratch.file()).await?;
    let initial = journal.revision().await?;
    journal.bind_engine("docker-engine-a").await?;
    let after_engine = journal.revision().await?;
    assert!(after_engine > initial);
    let LaunchReservation::New(id) = journal.reserve_assignment(7, 42, 100, 1).await? else {
        return Err(HostError::Journal);
    };
    let after_reservation = journal.revision().await?;
    assert!(after_reservation > after_engine);
    journal.finish(id, Outcome::Uncertain).await?;
    let after_finish = journal.revision().await?;
    assert!(after_finish > after_reservation);
    journal.record_cleanup(id).await?;
    assert!(journal.revision().await? > after_finish);
    Ok(())
}

#[tokio::test]
async fn a_failed_clean_assignment_gets_a_new_attempt_only_on_redelivery() -> Result<(), HostError>
{
    let scratch = Scratch::new("assignment-retry")?;
    let journal = open(&scratch.file()).await?;
    let LaunchReservation::New(first) = journal.reserve_assignment(7, 42, 100, 1).await? else {
        return Err(HostError::Journal);
    };
    journal.finish(first, Outcome::DefiniteFailure).await?;
    journal.record_cleanup(first).await?;
    assert_eq!(journal.occupied_launches().await?, 0);

    let LaunchReservation::New(retry) = journal.reserve_assignment(7, 42, 101, 1).await? else {
        return Err(HostError::Journal);
    };
    assert_ne!(retry, first);
    assert_eq!(journal.occupied_launches().await?, 1);
    assert_eq!(journal.rows().await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn a_clean_completed_assignment_stays_idempotent() -> Result<(), HostError> {
    let scratch = Scratch::new("assignment-completed")?;
    let journal = open(&scratch.file()).await?;
    let LaunchReservation::New(id) = journal.reserve_assignment(7, 42, 100, 1).await? else {
        return Err(HostError::Journal);
    };
    journal.finish(id, Outcome::Done).await?;
    journal.record_cleanup(id).await?;
    assert_eq!(
        journal.reserve_assignment(7, 42, 101, 1).await?,
        LaunchReservation::Existing(id)
    );
    assert_eq!(journal.rows().await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn acknowledgment_does_not_release_a_worker_slot() -> Result<(), HostError> {
    let scratch = Scratch::new("ack-slot")?;
    let journal = open(&scratch.file()).await?;
    let LaunchReservation::New(id) = journal.reserve_assignment(7, 42, 100, 1).await? else {
        return Err(HostError::Journal);
    };
    journal.finish(id, Outcome::Done).await?;
    assert_eq!(journal.occupied_launches().await?, 1);
    journal.record_cleanup(id).await?;
    assert_eq!(journal.occupied_launches().await?, 0);
    Ok(())
}
