use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::ConnectionId;
use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::capacity_header::model::{
    GenerationState, HeaderGeneration, HeaderLedgerError, HeaderSessionIdentity, PollOutcome,
    SessionId,
};
use crate::journal::connection::CapacityLimits;
use crate::journal::connection::storage::in_transaction;

use super::{
    MAX_SNAPSHOT_BYTES, bootstrap, check_snapshot_size, snapshot_digest, transition, validate,
    validate_capacity_policy_connection,
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
async fn lowering_host_limit_below_saved_charge_rolls_back_in_one_transaction() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 3).await?;
    let essential = connection("essential")?;
    transition(&journal, |ledger| {
        ledger.reserve(essential, 10, "session-a", 2)
    })
    .await?;

    let changed = in_transaction(&journal, async |connection| {
        connection
            .execute(
                "UPDATE host_capacity_policy SET max_jobs = 1 WHERE singleton = 1",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        validate_capacity_policy_connection(connection).await
    })
    .await;
    assert!(changed.is_err());
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
        ledger.finish_poll(&attempt, PollOutcome::Unknown)
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
            ledger.finish_poll(&request, PollOutcome::Response)
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
            ledger.finish_poll(&zero, PollOutcome::Response)
        })
        .await,
    )?;
    let deletion = at(
        "begin exact delete",
        transition(&journal, |ledger| ledger.begin_delete(&allocation)).await,
    )?;
    at(
        "record exact delete response",
        transition(&journal, |ledger| ledger.finish_delete(&deletion)).await,
    )?;
    drop(journal);

    let reopened = at("reopen after retirement", open_journal(&scratch, 2).await)?;
    let charged = at(
        "check final charge",
        transition(&reopened, |ledger| Ok(ledger.charged_capacity())).await,
    )?;
    assert_eq!(charged, 0);
    Ok(())
}

#[tokio::test]
async fn reopen_after_delete_intent_keeps_charge_and_blocks_replay() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 2).await?;
    let essential = connection("essential")?;
    let allocation = transition(&journal, |ledger| {
        ledger.reserve(essential, 10, "session-a", 2)
    })
    .await?;
    let zero = transition(&journal, |ledger| ledger.begin_zero_poll(&allocation)).await?;
    transition(&journal, |ledger| {
        ledger.finish_poll(&zero, PollOutcome::Response)
    })
    .await?;
    transition(&journal, |ledger| ledger.begin_delete(&allocation)).await?;
    drop(journal);

    let reopened = open_journal(&scratch, 2).await?;
    let (charged, retry) = transition(&reopened, |ledger| {
        Ok((ledger.charged_capacity(), ledger.begin_delete(&allocation)))
    })
    .await?;
    assert_eq!(charged, 2);
    assert_eq!(retry, Err(HeaderLedgerError::DeleteOutcomeUncertain));
    Ok(())
}

#[test]
fn previous_snapshot_version_is_strictly_decoded_for_recovery() -> TestResult {
    let previous = super::HeaderLedgerSnapshotV1 {
        max_jobs: 2,
        next_generation: 2,
        records: vec![super::HeaderRecordV1 {
            identity: HeaderSessionIdentity {
                connection_id: connection("essential")?,
                scale_set_id: 10,
                session_id: SessionId("session-a".to_owned()),
            },
            generation: HeaderGeneration(1),
            target: 2,
            state: GenerationState::Pending,
            attempt_sequence: 0,
            in_flight_poll: None,
            zero_header_settled: false,
        }],
    };
    let bytes = serde_json::to_vec(&previous)?;
    let json = std::str::from_utf8(&bytes)?;
    let digest = snapshot_digest(&bytes);
    let ledger = super::decode_snapshot(1, json, &digest, true)?;
    assert_eq!(ledger.charged_capacity(), 2);

    let mut malformed = serde_json::to_value(previous)?;
    malformed["records"][0]["unreviewed"] = serde_json::Value::Bool(true);
    let malformed_bytes = serde_json::to_vec(&malformed)?;
    let malformed_json = std::str::from_utf8(&malformed_bytes)?;
    let malformed_digest = snapshot_digest(&malformed_bytes);
    assert!(super::decode_snapshot(1, malformed_json, &malformed_digest, true).is_err());
    Ok(())
}

#[tokio::test]
async fn corrupt_persisted_metadata_fails_closed() -> TestResult {
    let scratch = Scratch::new()?;
    let journal = open_journal(&scratch, 2).await?;
    let essential = connection("essential")?;
    transition(&journal, |ledger| {
        ledger.reserve(essential, 10, "session-a", 2)
    })
    .await?;

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
            "UPDATE capacity_header_meta SET schema_version = 3, snapshot_digest = ?1 WHERE singleton = 1",
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
            "ALTER TABLE capacity_header_meta ADD COLUMN unexpected TEXT",
            (),
        )
        .await?;
    assert!(bootstrap(&journal).await.is_err());
    assert!(check_snapshot_size(&vec![b'x'; MAX_SNAPSHOT_BYTES]).is_ok());
    assert!(check_snapshot_size(&vec![b'x'; MAX_SNAPSHOT_BYTES + 1]).is_err());
    Ok(())
}
