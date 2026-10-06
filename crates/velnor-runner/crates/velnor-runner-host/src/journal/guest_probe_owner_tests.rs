//! Journal probe tokens survive restart while exclusive ownership does not.

use crate::error::HostError;
use crate::journal::{Journal, guest_probe_owner::lock_path};
use crate::launch_harness::Scratch;

#[tokio::test]
async fn same_journal_has_one_owner_and_reuses_its_token_after_release() -> Result<(), String> {
    let scratch = Scratch::new("guest-probe-owner-same").map_err(error_text)?;
    let path = scratch.file();
    let first_journal = Journal::open(&path).await.map_err(error_text)?;
    let first = first_journal
        .claim_guest_probe_owner()
        .await
        .map_err(error_text)?;
    let token = first.token().to_owned();
    let second_journal = Journal::open(&path).await.map_err(error_text)?;

    assert!(matches!(
        second_journal.claim_guest_probe_owner().await,
        Err(HostError::Lock)
    ));
    drop(first);
    let restarted_journal = Journal::open(&path).await.map_err(error_text)?;
    let restarted = restarted_journal
        .claim_guest_probe_owner()
        .await
        .map_err(error_text)?;

    assert_eq!(restarted.token(), token);
    Ok(())
}

#[tokio::test]
async fn distinct_journals_receive_distinct_owner_tokens() -> Result<(), String> {
    let first_scratch = Scratch::new("guest-probe-owner-a").map_err(error_text)?;
    let second_scratch = Scratch::new("guest-probe-owner-b").map_err(error_text)?;
    let first_journal = Journal::open(&first_scratch.file())
        .await
        .map_err(error_text)?;
    let second_journal = Journal::open(&second_scratch.file())
        .await
        .map_err(error_text)?;
    let first = first_journal
        .claim_guest_probe_owner()
        .await
        .map_err(error_text)?;
    let second = second_journal
        .claim_guest_probe_owner()
        .await
        .map_err(error_text)?;

    assert_ne!(first.token(), second.token());
    Ok(())
}

#[tokio::test]
async fn malformed_durable_owner_state_is_rejected() -> Result<(), String> {
    let scratch = Scratch::new("guest-probe-owner-malformed").map_err(error_text)?;
    let journal = Journal::open(&scratch.file()).await.map_err(error_text)?;
    let connection = journal.connection().await.map_err(error_text)?;
    connection
        .execute("DROP TABLE guest_probe_owner", ())
        .await
        .map_err(error_text)?;
    connection
        .execute(
            "CREATE TABLE guest_probe_owner (id INTEGER PRIMARY KEY, owner_token TEXT NOT NULL)",
            (),
        )
        .await
        .map_err(error_text)?;
    connection
        .execute(
            "INSERT INTO guest_probe_owner (id, owner_token) VALUES (1, 'not-a-token')",
            (),
        )
        .await
        .map_err(error_text)?;

    assert!(matches!(
        journal.guest_probe_owner_token().await,
        Err(HostError::Journal)
    ));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_probe_lock_is_rejected_without_following_it() -> Result<(), String> {
    let scratch = Scratch::new("guest-probe-owner-symlink").map_err(error_text)?;
    let path = scratch.file();
    let journal = Journal::open(&path).await.map_err(error_text)?;
    let canonical = std::fs::canonicalize(&path).map_err(error_text)?;
    let lock_path = lock_path(&canonical).map_err(error_text)?;
    let target = scratch.file().with_extension("foreign-lock");
    std::fs::write(&target, b"foreign").map_err(error_text)?;
    std::os::unix::fs::symlink(&target, &lock_path).map_err(error_text)?;

    let claim = journal.claim_guest_probe_owner().await;

    assert!(matches!(claim, Err(HostError::Lock)));
    assert_eq!(std::fs::read(&target).map_err(error_text)?, b"foreign");
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_journal_path_is_rejected() -> Result<(), String> {
    let scratch = Scratch::new("guest-probe-owner-db-symlink").map_err(error_text)?;
    let path = scratch.file();
    let journal = Journal::open(&path).await.map_err(error_text)?;
    let alias = path.with_file_name("journal-alias.db");
    std::os::unix::fs::symlink(&path, &alias).map_err(error_text)?;

    assert!(Journal::open(&alias).await.is_err());
    assert!(journal.claim_guest_probe_owner().await.is_ok());
    Ok(())
}

#[tokio::test]
async fn replaced_database_identity_is_rejected_by_an_existing_journal() -> Result<(), String> {
    let scratch = Scratch::new("guest-probe-owner-db-replaced").map_err(error_text)?;
    let path = scratch.file();
    let existing = Journal::open(&path).await.map_err(error_text)?;
    let original = existing
        .claim_guest_probe_owner()
        .await
        .map_err(error_text)?;
    drop(original);
    let backup = path.with_file_name("journal-old.db");
    std::fs::rename(&path, &backup).map_err(error_text)?;
    Journal::open(&path).await.map_err(error_text)?;

    assert!(existing.claim_guest_probe_owner().await.is_err());
    Ok(())
}

fn error_text(error: impl ToString) -> String {
    error.to_string()
}
