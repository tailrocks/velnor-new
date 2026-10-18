use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::ConnectionId;
use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::capacity_header::model::{
    GenerationState, HeaderLedgerError, PollOutcome, PollsSettledProof, SessionDeleteProof,
};
use crate::journal::connection::CapacityLimits;

use super::{
    MAX_SNAPSHOT_BYTES, bootstrap, check_snapshot_size, snapshot_digest, transition, validate,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
type TestResult = Result<(), Box<dyn Error>>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-header-storage-{}-{sequence}",
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
        let _ = std::fs::remove_dir_all(&self.0);
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
    bootstrap(&journal).await?;
    Ok(journal)
}

fn at<T>(stage: &'static str, result: Result<T, HostError>) -> Result<T, std::io::Error> {
    result.map_err(|error| std::io::Error::other(format!("{stage}: {error:?}")))
}

fn connection(value: &str) -> Result<ConnectionId, HostError> {
    ConnectionId::parse(value)
}

#[tokio::test]
async fn reopen_keeps_interrupted_poll_uncertain_and_charged() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 3).await?;
    let essential = connection("essential")?;
    let allocation = transition(&journal, |ledger| {
        ledger.reserve(essential, 10, "session-a", 2)
    })
    .await?;
    transition(&journal, |ledger| {
        ledger.begin_poll(&allocation).map(|_| ())
    })
    .await?;
    drop(journal);

    let reopened = open_journal(&scratch, 3).await?;
    let (charged, state, retry) = transition(&reopened, |ledger| {
        let state = ledger.snapshot().records[0].state;
        Ok((
            ledger.charged_capacity(),
            state,
            ledger.begin_poll(&allocation),
        ))
    })
    .await?;
    assert_eq!(charged, 2);
    assert_eq!(state, GenerationState::Uncertain);
    assert_eq!(retry, Err(HeaderLedgerError::UnsettledPoll));
    Ok(())
}

#[tokio::test]
async fn lowering_host_limit_below_saved_charge_fails_closed() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 3).await?;
    let essential = connection("essential")?;
    transition(&journal, |ledger| {
        ledger.reserve(essential, 10, "session-a", 2)
    })
    .await?;

    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await?;
    assert!(validate(&journal).await.is_err());
    assert!(bootstrap(&journal).await.is_err());
    assert!(
        transition(&journal, |ledger| Ok(ledger.charged_capacity()))
            .await
            .is_err()
    );

    journal
        .configure_capacity(CapacityLimits {
            jobs: 3,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await?;
    validate(&journal).await?;
    assert_eq!(
        transition(&journal, |ledger| Ok(ledger.charged_capacity())).await?,
        2
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_connections_cannot_reserve_more_than_host_max_jobs() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 3).await?;
    let second = open_journal(&scratch, 3).await?;
    let essential = connection("essential")?;
    let chainargos = connection("chainargos")?;
    let first = transition(&journal, |ledger| {
        ledger.reserve(essential, 10, "session-a", 2)
    });
    let second = transition(&second, |ledger| {
        ledger.reserve(chainargos, 20, "session-b", 2)
    });
    let (first, second) = tokio::join!(first, second);
    assert_ne!(first.is_ok(), second.is_ok());
    let charged = transition(&journal, |ledger| Ok(ledger.charged_capacity())).await?;
    assert_eq!(charged, 2);
    Ok(())
}

#[tokio::test]
async fn duplicate_reservation_is_idempotent_and_uncertainty_blocks_replacement() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 3).await?;
    let essential = connection("essential")?;
    let allocation = transition(&journal, |ledger| {
        ledger.reserve(essential.clone(), 10, "session-a", 2)
    })
    .await?;
    let repeated = transition(&journal, |ledger| {
        ledger.reserve(essential.clone(), 10, "session-a", 2)
    })
    .await?;
    assert_eq!(allocation, repeated);

    let attempt = transition(&journal, |ledger| ledger.begin_poll(&allocation)).await?;
    transition(&journal, |ledger| {
        ledger.finish_poll(attempt, PollOutcome::Unknown)
    })
    .await?;
    let changed_target = transition(&journal, |ledger| {
        ledger.reserve(essential.clone(), 10, "session-a", 1)
    })
    .await;
    let changed_session = transition(&journal, |ledger| {
        ledger.reserve(essential, 10, "session-b", 2)
    })
    .await;
    assert!(changed_target.is_err());
    assert!(changed_session.is_err());
    let charged = transition(&journal, |ledger| Ok(ledger.charged_capacity())).await?;
    assert_eq!(charged, 2);
    Ok(())
}

#[tokio::test]
async fn settled_zero_poll_and_exact_delete_are_durable_across_reopen() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = at("open initial journal", open_journal(&scratch, 2).await)?;
    let essential = connection("essential")?;
    let allocation = at(
        "reserve",
        transition(&journal, |ledger| {
            ledger.reserve(essential, 10, "session-a", 2)
        })
        .await,
    )?;
    let request = at(
        "begin advertise",
        transition(&journal, |ledger| ledger.begin_poll(&allocation)).await,
    )?;
    at(
        "finish advertise",
        transition(&journal, |ledger| {
            ledger.finish_poll(request, PollOutcome::Response)
        })
        .await,
    )?;
    let zero = at(
        "begin zero",
        transition(&journal, |ledger| ledger.begin_zero_poll(&allocation)).await,
    )?;
    at(
        "finish zero",
        transition(&journal, |ledger| {
            ledger.finish_poll(zero, PollOutcome::Response)
        })
        .await,
    )?;
    let polls = PollsSettledProof::test_only(&allocation);
    let deletion = SessionDeleteProof::test_only(&allocation);
    drop(journal);

    let reopened = at("reopen before retirement", open_journal(&scratch, 2).await)?;
    let before_retirement = at(
        "release",
        transition(&reopened, |ledger| {
            let charged = ledger.charged_capacity();
            ledger.release(&polls, &deletion)?;
            Ok(charged)
        })
        .await,
    )?;
    assert_eq!(before_retirement, 2);
    drop(reopened);

    let after_delete = at("reopen after delete", open_journal(&scratch, 2).await)?;
    let charged = at(
        "check final charge",
        transition(&after_delete, |ledger| Ok(ledger.charged_capacity())).await,
    )?;
    assert_eq!(charged, 0);
    Ok(())
}

#[tokio::test]
async fn forged_release_pair_and_corrupt_persisted_metadata_fail_closed() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 2).await?;
    let essential = connection("essential")?;
    let chainargos = connection("chainargos")?;
    let first = transition(&journal, |ledger| {
        ledger.reserve(essential, 10, "session-a", 1)
    })
    .await?;
    let second = transition(&journal, |ledger| {
        ledger.reserve(chainargos, 20, "session-b", 1)
    })
    .await?;
    let polls = PollsSettledProof::test_only(&first);
    let wrong_delete = SessionDeleteProof::test_only(&second);
    assert!(
        transition(&journal, |ledger| ledger.release(&polls, &wrong_delete))
            .await
            .is_err()
    );
    assert_eq!(
        transition(&journal, |ledger| Ok(ledger.charged_capacity())).await?,
        2
    );

    let connection = journal.connection().await?;
    connection
        .execute(
            "UPDATE capacity_header_meta SET snapshot_digest = ?1 WHERE singleton = 1",
            ["0".repeat(64)],
        )
        .await?;
    assert!(validate(&journal).await.is_err());
    assert!(transition(&journal, |_| Ok(())).await.is_err());
    let mut rows = connection
        .query(
            "SELECT snapshot_json FROM capacity_header_meta WHERE singleton = 1",
            (),
        )
        .await?;
    let snapshot_json: String = rows
        .next()
        .await?
        .ok_or_else(|| std::io::Error::other("header snapshot missing"))?
        .get(0)?;
    let digest = snapshot_digest(snapshot_json.as_bytes());
    connection
        .execute(
            "UPDATE capacity_header_meta SET schema_version = 2, snapshot_digest = ?1 WHERE singleton = 1",
            [digest],
        )
        .await?;
    assert!(validate(&journal).await.is_err());
    Ok(())
}

#[tokio::test]
async fn unknown_snapshot_field_fails_with_matching_digest() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 2).await?;
    let essential = connection("essential")?;
    transition(&journal, |ledger| {
        ledger.reserve(essential, 10, "session-a", 1)
    })
    .await?;

    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT snapshot_json FROM capacity_header_meta WHERE singleton = 1",
            (),
        )
        .await?;
    let snapshot_json: String = rows
        .next()
        .await?
        .ok_or_else(|| std::io::Error::other("header snapshot missing"))?
        .get(0)?;
    let mut snapshot: serde_json::Value = serde_json::from_str(&snapshot_json)?;
    let object = snapshot
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("header snapshot is not an object"))?;
    object.insert("unexpected".to_owned(), serde_json::Value::Bool(true));
    let json = serde_json::to_string(&snapshot)?;
    let digest = snapshot_digest(json.as_bytes());
    connection
        .execute(
            "UPDATE capacity_header_meta SET snapshot_json = ?1, snapshot_digest = ?2 WHERE singleton = 1",
            (json, digest),
        )
        .await?;

    assert!(validate(&journal).await.is_err());
    Ok(())
}

#[tokio::test]
async fn bootstrap_rejects_noncanonical_table_and_snapshot_size_is_bounded() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.journal_path()).await?;
    let connection = journal.connection().await?;
    connection
        .execute(
            "CREATE TABLE capacity_header_meta (singleton INTEGER PRIMARY KEY, schema_version INTEGER, snapshot_json TEXT, snapshot_digest TEXT, unexpected TEXT)",
            (),
        )
        .await?;
    assert!(bootstrap(&journal).await.is_err());
    assert!(check_snapshot_size(&vec![b'x'; MAX_SNAPSHOT_BYTES]).is_ok());
    assert!(check_snapshot_size(&vec![b'x'; MAX_SNAPSHOT_BYTES + 1]).is_err());
    Ok(())
}
