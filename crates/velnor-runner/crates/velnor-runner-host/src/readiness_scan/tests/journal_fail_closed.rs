use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::super::{bounded_journal_scan, bounded_readiness, journal_mark};
use super::{JournalFact, Scratch, deadline};
use crate::error::HostError;
use crate::journal::Journal;
use crate::readiness::Readiness;

#[tokio::test]
async fn nonregular_and_broken_link_journals_are_unreadable() -> Result<(), String> {
    let scratch = Scratch::new("nonregular")?;
    let directory = scratch.path().join("directory.db");
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    if journal_mark(&directory, deadline()).await != JournalFact::Unreadable {
        return Err("directory journal was treated as absent".to_owned());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let broken = scratch.path().join("broken.db");
        symlink(scratch.path().join("missing-target.db"), &broken)
            .map_err(|error| error.to_string())?;
        if journal_mark(&broken, deadline()).await != JournalFact::Unreadable {
            return Err("broken symlink journal was treated as absent".to_owned());
        }
    }
    scratch.cleanup()?;
    Ok(())
}

#[tokio::test]
async fn malformed_cleanup_and_unknown_kinds_are_unreadable() -> Result<(), String> {
    let scratch = Scratch::new("metadata")?;
    let path = scratch.path().join("launch.db");
    let journal = Journal::open(&path).await.map_err(|_| "open")?;
    journal.begin("launch", "job").await.map_err(|_| "begin")?;
    drop(journal);
    let text = path.to_str().ok_or("utf8")?;
    let db = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| "build")?;
    let conn = db.connect().map_err(|_| "connect")?;
    conn.execute("UPDATE intents SET cleanup_proven = ?1", [2_i64])
        .await
        .map_err(|_| "update proof")?;
    drop(conn);
    if journal_mark(&path, deadline()).await != JournalFact::Unreadable {
        return Err("non-boolean cleanup proof was accepted".to_owned());
    }
    set_unknown_kind(text).await?;
    if journal_mark(&path, deadline()).await != JournalFact::Unreadable {
        return Err("unknown journal kind was accepted".to_owned());
    }
    scratch.cleanup()?;
    Ok(())
}

async fn set_unknown_kind(path: &str) -> Result<(), String> {
    let db = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| "reopen")?;
    let conn = db.connect().map_err(|_| "reconnect")?;
    conn.execute(
        "UPDATE intents SET cleanup_proven = ?1, kind = ?2",
        (0_i64, "future-kind"),
    )
    .await
    .map_err(|_| "update kind")?;
    drop(conn);
    Ok(())
}

struct Dropped(Arc<AtomicBool>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn readiness_deadline_degrades_and_drops_pending_work() {
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(Arc::clone(&dropped));
    let pending = async move {
        let _guard = guard;
        std::future::pending::<Readiness>().await
    };
    let state = bounded_readiness(
        pending,
        tokio::time::Instant::now() + std::time::Duration::from_millis(1),
    )
    .await;
    assert_eq!(state, Readiness::Degraded);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn journal_deadline_is_unreadable_and_drops_pending_query() {
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(Arc::clone(&dropped));
    let pending = async move {
        let _guard = guard;
        std::future::pending::<Result<bool, HostError>>().await
    };
    let mark = bounded_journal_scan(
        pending,
        tokio::time::Instant::now() + std::time::Duration::from_millis(1),
    )
    .await;
    assert_eq!(mark, JournalFact::Unreadable);
    assert!(dropped.load(Ordering::SeqCst));
}
