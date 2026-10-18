use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::ConnectionId;
use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::CapacityLimits;
use crate::launch::{CompletedHeaderPoll, DeletedSession204};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
type TestResult = Result<(), Box<dyn Error>>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-header-authority-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn journal_path(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("could not remove header test data: {error}"),
        }
    }
}

async fn open_journal(scratch: &Scratch, max_jobs: u32) -> Result<Journal, HostError> {
    let journal = Journal::open(&scratch.journal_path()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: max_jobs,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await?;
    journal.bootstrap_header_authority().await?;
    Ok(journal)
}

fn connection(value: &str) -> Result<ConnectionId, HostError> {
    ConnectionId::parse(value)
}

#[tokio::test]
async fn reservations_from_multiple_connections_share_one_durable_limit() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 3).await?;
    let first = journal
        .reserve_header_allocation(connection("essential")?, 10, "session-a", 2)
        .await?;
    let repeated = journal
        .reserve_header_allocation(connection("essential")?, 10, "session-a", 2)
        .await?;
    assert_eq!(first, repeated);
    assert!(
        journal
            .reserve_header_allocation(connection("chainargos")?, 20, "session-b", 2)
            .await
            .is_err()
    );
    assert_eq!(journal.charged_header_capacity().await?, 2);
    assert_eq!(journal.header_allocations().await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn committed_poll_intent_becomes_uncertain_after_reopen() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 3).await?;
    let allocation = journal
        .reserve_header_allocation(connection("essential")?, 10, "session-a", 2)
        .await?;
    let attempt = journal.begin_header_poll(&allocation).await?;
    assert_eq!(attempt.header_value(), 2);
    drop(journal);

    let reopened = open_journal(&scratch, 3).await?;
    assert_eq!(reopened.charged_header_capacity().await?, 2);
    assert!(reopened.begin_header_poll(&allocation).await.is_err());
    Ok(())
}

#[tokio::test]
async fn typed_poll_completion_zero_request_and_delete_retire_exact_session() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 2).await?;
    let allocation = journal
        .reserve_header_allocation(connection("essential")?, 10, "session-a", 2)
        .await?;
    let poll = journal.begin_header_poll(&allocation).await?;
    journal
        .settle_header_poll(CompletedHeaderPoll::test_only(poll))
        .await?;

    let zero = journal.begin_zero_header_poll(&allocation).await?;
    assert_eq!(zero.header_value(), 0);
    journal
        .settle_header_poll(CompletedHeaderPoll::test_only(zero))
        .await?;

    let deletion = journal.begin_header_session_delete(&allocation).await?;
    journal
        .retire_header_allocation(DeletedSession204::test_only(deletion))
        .await?;
    assert_eq!(journal.charged_header_capacity().await?, 0);
    Ok(())
}
