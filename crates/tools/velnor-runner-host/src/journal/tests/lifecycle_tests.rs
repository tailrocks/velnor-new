//! Durable controller drain state and non-mutating journal inspection.

use std::path::Path;

use crate::journal::LaunchClaim;
use crate::{HostError, Journal};

use super::Scratch;

fn directory_entries(path: &Path) -> Result<Vec<String>, String> {
    let mut entries = std::fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .map(|entry| {
            entry
                .map(|item| item.file_name().to_string_lossy().into_owned())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    Ok(entries)
}

#[tokio::test]
async fn readonly_open_reads_existing_journal_without_creating_sidecars() -> Result<(), String> {
    let scratch = Scratch::new("readonly-journal").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .begin_launch("launch-1")
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);

    let before = directory_entries(&scratch.path)?;
    let readonly = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        readonly
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    assert_eq!(readonly.draining().await, Ok(false));
    assert_eq!(readonly.request_drain().await, Err(HostError::Journal));
    drop(readonly);

    assert_eq!(directory_entries(&scratch.path)?, before);
    Ok(())
}

#[tokio::test]
async fn existing_journal_open_rejects_missing_file_without_creating_it() -> Result<(), String> {
    let scratch = Scratch::new("missing-journal").map_err(|error| error.to_string())?;
    let path = scratch.file();

    assert!(matches!(
        Journal::open_readonly(&path).await,
        Err(HostError::Journal)
    ));
    assert!(matches!(
        Journal::open_existing(&path).await,
        Err(HostError::Journal)
    ));
    assert!(!path.exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn existing_journal_open_rejects_symlink() -> Result<(), String> {
    let scratch = Scratch::new("symlink-journal").map_err(|error| error.to_string())?;
    let target = scratch.path.join("target.db");
    let link = scratch.file();
    Journal::open(&target)
        .await
        .map_err(|error| error.to_string())?;
    std::os::unix::fs::symlink(&target, &link).map_err(|error| error.to_string())?;

    assert!(matches!(
        Journal::open_readonly(&link).await,
        Err(HostError::Journal)
    ));
    assert!(matches!(
        Journal::open_existing(&link).await,
        Err(HostError::Journal)
    ));
    Ok(())
}

#[tokio::test]
async fn drain_intent_survives_reopen_and_resume_is_explicit() -> Result<(), String> {
    let scratch = Scratch::new("durable-drain").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(journal.draining().await, Ok(false));
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);

    let readonly = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(readonly.draining().await, Ok(true));
    drop(readonly);

    let writable = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    writable.resume().await.map_err(|error| error.to_string())?;
    assert_eq!(writable.draining().await, Ok(false));
    Ok(())
}

#[tokio::test]
async fn launch_claim_and_drain_cutoff_are_atomic_and_idempotent() -> Result<(), String> {
    let scratch = Scratch::new("drain-cutoff").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let claim = journal
        .begin_launch_if_accepting("before-drain")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(claim, LaunchClaim::New(1));
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .begin_launch_if_accepting("after-drain")
            .await
            .map_err(|error| error.to_string())?,
        LaunchClaim::Draining
    );
    assert_eq!(
        journal
            .begin_launch_if_accepting("before-drain")
            .await
            .map_err(|error| error.to_string())?,
        LaunchClaim::Existing(1)
    );
    Ok(())
}
