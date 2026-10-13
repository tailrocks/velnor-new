//! Completion effect fencing, cleanup bounds, retry signals, and burst scans.

#[path = "saturated_tests.rs"]
mod saturated_tests;
#[path = "shutdown_tests.rs"]
mod shutdown_tests;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use tokio::sync::oneshot;

use super::{
    CLAIM_LEASE_SECONDS, CURL_REQUEST_TIMEOUT_SECONDS, Context, EFFECT_MARGIN_SECONDS,
    EFFECT_WINDOW_SECONDS, EffectBudget, Failure, MAX_CLEANUP_EFFECTS, MAX_EXTERNAL_CHAIN_SECONDS,
    MAX_SCAN_WAVES, SCAN_LIMIT, cleanup_retry_delay_seconds, completion_now, reconcile_due_burst,
    run_effect,
};
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::Journal;
use crate::listen::Secret;

const TEST_REQUEST_ID: i64 = 61;
const SLOW_REQUEST_DURATION: Duration = Duration::from_millis(2_300);
const ORIGINAL_LEASE_CROSSING: Duration = Duration::from_millis(2_100);

#[tokio::test]
async fn slow_request_keeps_its_lease_after_the_original_claim_expires() -> Result<(), HostError> {
    let scratch = Scratch::new("effect-fence")?;
    let first = Journal::open(scratch.file()).await?;
    let second = Journal::open(scratch.file()).await?;
    let id = completed_launch(&first, TEST_REQUEST_ID).await?;
    let claim = first
        .claim_completion_cleanup(id, 2)
        .await?
        .ok_or(HostError::Journal)?;
    let (started_tx, started_rx) = oneshot::channel();
    let generation = claim.generation;
    let attempt = claim.attempt;
    let stopping = AtomicBool::new(false);
    let first_effect = first;
    let effect = tokio::spawn(async move {
        let mut budget = EffectBudget::new(&stopping);
        let outcome: Result<(), Failure> = run_effect(
            &first_effect,
            &mut budget,
            id,
            claim,
            "slow Docker request",
            || async move {
                started_tx.send(()).map_err(|()| HostError::Journal)?;
                tokio::time::sleep(SLOW_REQUEST_DURATION).await;
                Err(HostError::Docker)
            },
        )
        .await;
        let failure = outcome.err().ok_or(HostError::Journal)?;
        let retry_delay = cleanup_retry_delay_seconds(attempt);
        let scheduled = first_effect
            .retry_completion_cleanup(id, generation, retry_delay)
            .await?;
        Ok::<_, HostError>((failure, scheduled, retry_delay))
    });
    started_rx.await.map_err(|_| HostError::Journal)?;
    tokio::time::sleep(ORIGINAL_LEASE_CROSSING).await;
    assert!(second.claim_completion_cleanup(id, 10).await?.is_none());
    let (failure, scheduled, retry_delay) = effect.await.map_err(|_| HostError::Journal)??;
    assert_eq!(
        failure,
        Failure::request("slow Docker request", HostError::Docker)
    );
    assert!(scheduled);
    assert!(retry_delay >= EFFECT_WINDOW_SECONDS);
    assert!(second.claim_completion_cleanup(id, 10).await?.is_none());
    let retry_due = completion_now()?
        .checked_add(retry_delay)
        .ok_or(HostError::Journal)?;
    assert!(
        second
            .claim_completion_cleanup_at(id, retry_due, 10)
            .await?
            .is_some()
    );
    Ok(())
}

#[test]
fn effect_window_and_budget_cover_the_complete_cleanup_chain() {
    let docker_seconds = crate::docker_client::DOCKER_OPERATION_TIMEOUT.as_secs();
    let https_source = include_str!("../../https.rs");
    assert!(https_source.contains("max-time = {max_time_seconds}"));
    assert!(https_source.contains("max_time_seconds: 60"));
    assert!(https_source.contains("format!(\"max-time = {max_time_seconds}\")"));
    assert_eq!(
        HttpsTransport::new("https://api.github.com").map(|transport| transport.timeout_seconds()),
        Ok(60)
    );
    assert_eq!(CLAIM_LEASE_SECONDS, EFFECT_WINDOW_SECONDS);
    assert_eq!(
        u64::try_from(EFFECT_WINDOW_SECONDS).ok(),
        Some(CURL_REQUEST_TIMEOUT_SECONDS + EFFECT_MARGIN_SECONDS)
    );
    assert!(
        u64::try_from(EFFECT_WINDOW_SECONDS)
            .is_ok_and(|window| window >= docker_seconds + EFFECT_MARGIN_SECONDS)
    );
    assert_eq!(MAX_CLEANUP_EFFECTS, 24);
    assert_eq!(MAX_EXTERNAL_CHAIN_SECONDS, 390);
    assert_eq!(
        u64::try_from(EFFECT_WINDOW_SECONDS)
            .ok()
            .map(|window| MAX_CLEANUP_EFFECTS * window),
        Some(2_160)
    );
    assert_eq!(cleanup_retry_delay_seconds(1), EFFECT_WINDOW_SECONDS + 5);
    assert_eq!(cleanup_retry_delay_seconds(5), EFFECT_WINDOW_SECONDS + 80);

    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);
    for _ in 0..MAX_CLEANUP_EFFECTS {
        assert!(budget.reserve("test request").is_ok());
    }
    assert_eq!(
        budget.reserve("extra request"),
        Err(Failure {
            phase: "extra request",
            reason: "effect_bound",
            error: HostError::Journal,
            stop_requested: false,
        })
    );
}

#[tokio::test]
async fn burst_processes_later_rows_before_waiting_for_the_scan_interval() -> Result<(), HostError>
{
    let scratch = Scratch::new("bounded-burst")?;
    let mut harness = Harness::new(scratch.file()).await?;
    let mut expected_remaining = 0;
    let burst_limit = usize::try_from(SCAN_LIMIT).map_err(|_| HostError::Journal)? * MAX_SCAN_WAVES;
    for request_id in 1..=i64::try_from(burst_limit + 1).map_err(|_| HostError::Journal)? {
        expected_remaining = completed_launch(&harness.journal, request_id).await?;
    }
    let mut context = harness.context();
    let stopping = AtomicBool::new(false);
    let (processed, failure) = reconcile_due_burst(&mut context, &stopping)
        .await
        .map_err(|failure| failure.error)?;
    assert_eq!(processed, burst_limit);
    assert_eq!(failure, Some(Failure::not_proven("completion identity")));
    let due = harness
        .journal
        .due_completed_launches(completion_now()?, 32)
        .await?;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].intent.id, expected_remaining);
    Ok(())
}

#[tokio::test]
async fn shutdown_request_leaves_due_rows_unclaimed() -> Result<(), HostError> {
    let scratch = Scratch::new("burst-stop")?;
    let mut harness = Harness::new(scratch.file()).await?;
    let first_id = completed_launch(&harness.journal, 1).await?;
    let second_id = completed_launch(&harness.journal, 2).await?;
    let mut context = harness.context();
    let stopping = AtomicBool::new(true);
    let (processed, failure) = reconcile_due_burst(&mut context, &stopping)
        .await
        .map_err(|failure| failure.error)?;

    assert_eq!(processed, 0);
    assert_eq!(failure, None);
    let due = harness
        .journal
        .due_completed_launches(completion_now()?, 32)
        .await?;
    assert_eq!(due.len(), 2);
    assert_eq!(due[0].intent.id, first_id);
    assert_eq!(due[1].intent.id, second_id);
    Ok(())
}

#[tokio::test]
async fn request_failure_signal_survives_a_successful_retry_schedule() -> Result<(), HostError> {
    let scratch = Scratch::new("request-failure")?;
    let journal = Journal::open(scratch.file()).await?;
    let id = completed_launch(&journal, TEST_REQUEST_ID).await?;
    let claim = journal
        .claim_completion_cleanup(id, CLAIM_LEASE_SECONDS)
        .await?
        .ok_or(HostError::Journal)?;
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);
    let failure = match run_effect(
        &journal,
        &mut budget,
        id,
        claim,
        "container delete",
        || async { Err::<(), _>(HostError::Docker) },
    )
    .await
    {
        Ok(()) => return Err(HostError::Journal),
        Err(failure) => failure,
    };
    assert_eq!(
        failure,
        Failure::request("container delete", HostError::Docker)
    );
    failure.report();
    assert!(
        journal
            .retry_completion_cleanup(id, claim.generation, 5)
            .await?
    );
    assert_eq!(failure.reason, "docker");
    Ok(())
}

struct Scratch {
    path: PathBuf,
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "velnor-completion-effect-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).map_err(|_| HostError::Path)?;
        Ok(Self {
            path: directory.join("journal.db"),
            directory,
        })
    }

    fn file(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.directory) {
            Ok(()) => {}
            Err(error) => {
                let _kind = error.kind();
            }
        }
    }
}

async fn completed_launch(journal: &Journal, request_id: i64) -> Result<i64, HostError> {
    let subject = format!("m{request_id}r{request_id}");
    let runner_name = format!("v{request_id}");
    let (id, _) = journal
        .begin_assigned_launch(&subject, 77, request_id, &runner_name)
        .await?;
    let recorded = journal
        .record_runner_completed(77, request_id, request_id + 10_000, &runner_name)
        .await?
        .ok_or(HostError::Journal)?;
    if recorded != id {
        return Err(HostError::Journal);
    }
    Ok(id)
}

struct Harness {
    journal: Journal,
    docker: Docker,
    transport: HttpsTransport,
    admin: Secret,
    _socket: SocketFile,
}

impl Harness {
    async fn new(path: &std::path::Path) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let socket_path = std::env::temp_dir().join(format!(
            "velnor-completion-test-{}-{id}.sock",
            std::process::id()
        ));
        let socket = SocketFile::new(socket_path)?;
        let socket_text = socket.path().to_str().ok_or(HostError::Path)?;
        let docker = Docker::connect_with_unix(socket_text, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|_| HostError::Docker)?;
        Ok(Self {
            journal: Journal::open(path).await?,
            docker,
            transport: HttpsTransport::new("https://github.com")?,
            admin: Secret::new("test token"),
            _socket: socket,
        })
    }

    fn context(&mut self) -> Context<'_> {
        Context {
            journal: &self.journal,
            docker: &self.docker,
            transport: &mut self.transport,
            admin: &self.admin,
        }
    }
}

struct SocketFile(PathBuf);

impl SocketFile {
    fn new(path: PathBuf) -> Result<Self, HostError> {
        std::fs::write(&path, b"").map_err(|_| HostError::Path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.0) {
            Ok(()) => {}
            Err(error) => {
                let _kind = error.kind();
            }
        }
    }
}
