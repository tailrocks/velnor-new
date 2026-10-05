//! Journal durability. Each call commits before it returns.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use velnor_runner_core::{
    AcquireIntentId, Capacity, CleanupProof, Epoch, OwnedIds, WorkerId, WorkerState,
};

use crate::reconcile::{before_advertise, occupies, release_permitted};
use crate::{HostError, IntentRow, IntentState, Journal, Outcome, Reconcile, ReleaseFact};

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-host-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self { path })
    }

    fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let cleanup = std::fs::remove_dir_all(&self.path);
        let _kept = cleanup.err().map(|err| err.kind());
    }
}

async fn open(path: &Path) -> Result<Journal, HostError> {
    Journal::open(path).await
}

struct Seen {
    state: Option<IntentState>,
    secret_ok: bool,
}

async fn observed_state(path: &Path) -> Option<IntentState> {
    let other = Journal::open(path).await.ok()?;
    let rows = other.rows().await.ok()?;
    rows.first().map(|row| row.state)
}

fn dir_contains(dir: &Path, needle: &str) -> Result<bool, HostError> {
    let raw = needle.as_bytes();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = std::fs::read_dir(&current).map_err(|_| HostError::Journal)?;
        for entry in entries {
            let path = entry.map_err(|_| HostError::Journal)?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let bytes = std::fs::read(&path).map_err(|_| HostError::Journal)?;
            if bytes.windows(raw.len()).any(|window| window == raw) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn intent(state: IntentState, kind: &str) -> IntentRow {
    IntentRow {
        id: 1,
        kind: kind.to_owned(),
        subject: "job".to_owned(),
        state,
        docker_id: None,
        dind_id: None,
        worker_volume: None,
        github_runner_id: None,
        cleanup_proven: false,
    }
}

#[tokio::test]
async fn reopen_sees_the_row() -> Result<(), HostError> {
    let scratch = Scratch::new("reopen")?;
    let path = scratch.file();
    let id = {
        let journal = open(&path).await?;
        journal.begin("provision", "job-1").await?
    };
    let again = open(&path).await?;
    assert_eq!(again.read(id).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn uncertain_outcome_keeps_the_row() -> Result<(), HostError> {
    let scratch = Scratch::new("uncertain")?;
    let journal = open(&scratch.file()).await?;
    let held = journal.begin("acquire", "job-1").await?;
    journal.finish(held, Outcome::Uncertain).await?;
    let next = journal.begin("provision", "job-2").await?;
    assert_eq!(journal.read(held).await?, IntentState::Uncertain);
    assert_eq!(journal.read(next).await?, IntentState::Pending);
    drop(journal);
    let again = open(&scratch.file()).await?;
    assert_eq!(again.read(held).await?, IntentState::Uncertain);
    Ok(())
}

#[tokio::test]
async fn missing_row_is_not_a_successful_finish() -> Result<(), HostError> {
    let scratch = Scratch::new("missing")?;
    let journal = open(&scratch.file()).await?;
    assert_eq!(
        journal.finish(99, Outcome::Done).await,
        Err(HostError::Journal)
    );
    let kept = journal.begin("provision", "keep").await?;
    let id = journal.begin("acquire", "drop-me").await?;
    journal.finish(id, Outcome::DefiniteFailure).await?;
    assert_eq!(journal.read(id).await?, IntentState::Failed);
    assert_eq!(journal.read(kept).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn empty_and_quoted_kinds_are_rejected() -> Result<(), HostError> {
    let scratch = Scratch::new("kind")?;
    let journal = open(&scratch.file()).await?;
    assert_eq!(journal.begin("", "job").await, Err(HostError::Journal));
    assert_eq!(journal.begin("it's", "job").await, Err(HostError::Journal));
    assert_eq!(
        journal.begin("say \"hi\"", "job").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.begin("provision", "").await,
        Err(HostError::Journal)
    );
    let id = journal.begin("provision", "job-1").await?;
    assert_eq!(journal.read(id).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn drop_does_not_delete_pending_rows() -> Result<(), HostError> {
    let scratch = Scratch::new("drop")?;
    let path = scratch.file();
    let id = {
        let journal = open(&path).await?;
        let id = journal.begin("provision", "job-1").await?;
        let clone = journal.clone();
        drop(clone);
        drop(journal);
        id
    };
    assert!(path.is_file());
    let again = open(&path).await?;
    assert_eq!(again.read(id).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn around_commits_pending_before_effect_and_hides_secret() -> Result<(), HostError> {
    const CANARY: &str = "velnor-canary-jit-pat-admin";
    let scratch = Scratch::new("around")?;
    let path = scratch.file();
    let journal = open(&path).await?;
    let seen = Arc::new(Mutex::new(Seen {
        state: None,
        secret_ok: false,
    }));
    let slot = Arc::clone(&seen);
    let probe = path.clone();
    let id = journal
        .around("acquire", "job-1", CANARY, async move |secret| {
            let secret_ok = secret == CANARY;
            let state = observed_state(&probe).await;
            if let Ok(mut guard) = slot.lock() {
                guard.secret_ok = secret_ok;
                guard.state = state;
            }
            Outcome::Uncertain
        })
        .await?;
    {
        let guard = seen.lock().map_err(|_| HostError::Journal)?;
        assert!(guard.secret_ok);
        assert_eq!(guard.state, Some(IntentState::Pending));
    }
    assert_eq!(journal.read(id).await?, IntentState::Uncertain);
    let again = open(&path).await?;
    assert_eq!(again.read(id).await?, IntentState::Uncertain);
    assert!(!dir_contains(&scratch.path, CANARY)?);
    journal
        .bind(id, Some("ctr-bind-1"), Some("gh-bind-9"))
        .await?;
    assert!(dir_contains(&scratch.path, "ctr-bind-1")?);
    Ok(())
}

#[tokio::test]
async fn replay_reuses_live_subject_and_failed_starts_again() -> Result<(), HostError> {
    let scratch = Scratch::new("subject")?;
    let journal = open(&scratch.file()).await?;
    let first = journal.begin("acquire", "req-7").await?;
    assert_eq!(journal.begin("acquire", "req-7").await?, first);
    journal.finish(first, Outcome::Done).await?;
    assert_eq!(journal.begin("acquire", "req-7").await?, first);
    journal.finish(first, Outcome::DefiniteFailure).await?;
    let third = journal.begin("acquire", "req-7").await?;
    assert_ne!(third, first);
    assert_eq!(journal.rows().await?.len(), 2);
    let other = journal.begin("provision", "req-7").await?;
    assert_ne!(other, third);
    let held = journal.begin("acquire", "req-9").await?;
    journal.finish(held, Outcome::Uncertain).await?;
    assert_eq!(journal.begin("acquire", "req-9").await?, held);
    Ok(())
}

#[tokio::test]
async fn cleanup_proof_reopens() -> Result<(), HostError> {
    let scratch = Scratch::new("proof")?;
    let path = scratch.file();
    let id = {
        let journal = open(&path).await?;
        let id = journal.begin("delete", "ctr-1").await?;
        journal.finish(id, Outcome::Done).await?;
        journal.bind(id, Some("ctr-1"), Some("gh-1")).await?;
        journal.record_cleanup(id).await?;
        id
    };
    let rows = open(&path).await?.rows().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert!(rows[0].cleanup_proven);
    assert_eq!(rows[0].docker_id.as_deref(), Some("ctr-1"));
    assert_eq!(rows[0].github_runner_id.as_deref(), Some("gh-1"));
    Ok(())
}

fn apply_release(
    capacity: &mut Capacity,
    worker: WorkerId,
    fact: ReleaseFact,
    proof: &CleanupProof,
) -> Result<(), HostError> {
    if release_permitted(fact) {
        capacity
            .release(worker, proof)
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

#[test]
fn release_permitted_gates_capacity_release() -> Result<(), HostError> {
    let worker = WorkerId::new(1).map_err(|_| HostError::Journal)?;
    let intent_id = AcquireIntentId::new(1).map_err(|_| HostError::Journal)?;
    let mut capacity = Capacity::new(1);
    capacity
        .reserve(worker, intent_id, Epoch::new(1), 1, 1)
        .map_err(|_| HostError::Journal)?;
    let owned = OwnedIds {
        container_id: "c1".to_owned(),
        volume: "v1".to_owned(),
    };
    capacity
        .store(
            worker,
            WorkerState::Cleaning {
                epoch: Epoch::new(1),
                owned: owned.clone(),
            },
        )
        .map_err(|_| HostError::Journal)?;
    let proof = CleanupProof {
        container_id: "c1".to_owned(),
        volume: "v1".to_owned(),
    };
    assert!(!release_permitted(ReleaseFact::Pending));
    assert!(!release_permitted(ReleaseFact::Uncertain));
    assert!(!release_permitted(ReleaseFact::DefiniteFailure));
    assert!(release_permitted(ReleaseFact::ProvenCleanup));
    apply_release(&mut capacity, worker, ReleaseFact::Pending, &proof)?;
    apply_release(&mut capacity, worker, ReleaseFact::Uncertain, &proof)?;
    apply_release(&mut capacity, worker, ReleaseFact::DefiniteFailure, &proof)?;
    assert_eq!(capacity.occupancy(), 1);
    apply_release(&mut capacity, worker, ReleaseFact::ProvenCleanup, &proof)?;
    assert_eq!(capacity.occupancy(), 0);
    Ok(())
}

#[test]
fn before_advertise_holds_uncertain_and_adopts() {
    let pending = intent(IntentState::Pending, "acquire");
    let uncertain = intent(IntentState::Uncertain, "acquire");
    assert_eq!(
        before_advertise(&[pending], &[], &[], &[]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
    assert_eq!(
        before_advertise(&[uncertain], &[], &[], &[]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
    let mut missing = intent(IntentState::Done, "delete");
    missing.docker_id = Some("ctr-missing".to_owned());
    missing.github_runner_id = Some("gh-1".to_owned());
    assert_eq!(
        before_advertise(&[missing.clone()], &[], &["gh-1"], &["ctr-missing"]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
    missing.docker_id = Some("ctr-seen".to_owned());
    assert_eq!(
        before_advertise(&[missing], &["ctr-seen"], &[], &["ctr-seen"]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
    assert_eq!(
        before_advertise(&[], &["ctr-owned", "ctr-foreign"], &[], &["ctr-owned"]),
        Reconcile::Hold {
            adopt: vec!["ctr-owned".to_owned()],
            occupied: 0,
        }
    );
    let mut settled = intent(IntentState::Done, "delete");
    settled.docker_id = Some("ctr-seen".to_owned());
    settled.github_runner_id = Some("gh-1".to_owned());
    assert_eq!(
        before_advertise(
            &[settled.clone()],
            &["ctr-seen", "ctr-owned"],
            &["gh-1"],
            &["ctr-seen", "ctr-owned"],
        ),
        Reconcile::Hold {
            adopt: vec!["ctr-owned".to_owned()],
            occupied: 1,
        }
    );
    assert_eq!(
        before_advertise(&[settled], &["ctr-seen"], &["gh-1"], &["ctr-seen"]),
        Reconcile::Advertise { occupied: 1 }
    );
}

#[test]
fn failed_acquire_does_not_occupy_and_clean_rows_advertise() {
    let failed = intent(IntentState::Failed, "acquire");
    assert!(!occupies(&failed));
    assert_eq!(
        before_advertise(&[failed], &[], &[], &[]),
        Reconcile::Advertise { occupied: 0 }
    );
    let done = intent(IntentState::Done, "acquire");
    assert!(occupies(&done));
    assert_eq!(
        before_advertise(&[done], &[], &[], &[]),
        Reconcile::Advertise { occupied: 1 }
    );
    let mut proved = intent(IntentState::Done, "delete");
    proved.docker_id = Some("ctr-gone".to_owned());
    proved.cleanup_proven = true;
    assert!(!occupies(&proved));
    assert_eq!(
        before_advertise(&[proved], &[], &[], &[]),
        Reconcile::Advertise { occupied: 0 }
    );
    let mut failed_delete = intent(IntentState::Failed, "delete");
    failed_delete.docker_id = Some("ctr-still".to_owned());
    assert!(occupies(&failed_delete));
    assert_eq!(
        before_advertise(&[failed_delete], &[], &[], &["ctr-still"]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
}
