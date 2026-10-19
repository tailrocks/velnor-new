//! Completion claim fencing, retry, renewal, and proof tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::reconcile::occupies;
use crate::{HostError, Journal, Outcome};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-completion-claim-{label}-{}-{id}",
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
        let cleanup = std::fs::remove_dir_all(&self.0);
        let _ignored = cleanup.err().map(|error| error.kind());
    }
}

async fn completed_launch(journal: &Journal) -> Result<i64, HostError> {
    let (id, _) = journal
        .begin_assigned_launch("m1r61", 77, 61, "v61")
        .await?;
    assert_eq!(
        journal.record_runner_completed(77, 61, 901, "v61").await?,
        Some(id)
    );
    Ok(id)
}

async fn set_claim_generation(
    path: &std::path::Path,
    id: i64,
    generation: i64,
) -> Result<(), HostError> {
    let text = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE completion_cleanup SET claim_generation = ?1 WHERE intent_id = ?2",
            (generation, id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn expire_claim(path: &std::path::Path, id: i64) -> Result<(), HostError> {
    let text = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE completion_cleanup SET lease_until = 0 WHERE intent_id = ?1",
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

#[path = "journal_completion_claim_tests/claim_cases.rs"]
mod claim_cases;
#[path = "journal_completion_claim_tests/effect_cases.rs"]
mod effect_cases;
