// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    cell::Cell,
    time::{Duration, Instant},
};

use super::{
    RESTORE_ATTEMPT_DEADLINE, RESTORE_RETRY, RestoreMode, guarded_restore,
    release_suspect_credentials, restore_bound_session, restore_missing,
};
use crate::{
    config::HostConfig,
    error::HostError,
    journal::{ConnectionJournal, Journal},
    scale_set::EnsureError,
};

#[tokio::test]
async fn present_credentials_are_not_reloaded() -> Result<(), ()> {
    let mut slot = Some(1_u32);
    restore_missing(&mut slot, async || -> Result<u32, ()> { Err(()) }).await?;
    assert_eq!(slot, Some(1));
    Ok(())
}

#[tokio::test]
async fn a_failed_restore_keeps_the_poll_held() {
    let mut slot: Option<u32> = None;
    assert!(
        restore_missing(&mut slot, async || -> Result<u32, ()> { Err(()) })
            .await
            .is_err()
    );
    assert!(slot.is_none());
}

const CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

const BINDING: &str = "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES ('conn-test-1', 7, 'ubuntu-26.04-scale-set', 'created', 'engine-test', 'unix:///docker.sock', 'linux/amd64')";
const POLICY: &str = "INSERT INTO host_capacity_policy (singleton, max_jobs, max_cpu_millicores, max_memory_bytes, operational_cpu_millicores, operational_memory_bytes) VALUES (1, 1, 100000, 1000000000000, 0, 0)";
const NONCE: &str = "0123456789abcdef0123456789abcdef";

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

async fn scope(dir: &tempfile::TempDir) -> TestResult<ConnectionJournal> {
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    journal.bind_engine("engine-test").await?;
    let config = HostConfig::parse(CONFIG)?;
    let binding = config.connections().first().ok_or("connection")?;
    let scope = journal.connection_scope(binding).await?;
    let connection = journal.connection().await?;
    connection.execute(BINDING, ()).await?;
    connection.execute(POLICY, ()).await?;
    Ok(scope)
}

async fn restored(scope: &ConnectionJournal, calls: &mut u32) -> Result<u32, HostError> {
    restore_bound_session(scope, 7, async || {
        *calls += 1;
        Ok::<_, HostError>(1)
    })
    .await
}

#[tokio::test]
async fn restore_refuses_when_no_bound_session_exists() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let scope = scope(&dir).await?;
    let mut calls = 0;
    assert!(restored(&scope, &mut calls).await.is_err());
    scope.prepare_session_create(NONCE).await?;
    assert!(restored(&scope, &mut calls).await.is_err());
    let claim = scope.claim_session_create(NONCE).await?.ok_or("claim")?;
    assert!(restored(&scope, &mut calls).await.is_err());
    assert_eq!(calls, 0);
    scope.record_created_session(&claim, "session-1", 1).await?;
    assert_eq!(restored(&scope, &mut calls).await?, 1);
    assert_eq!(calls, 1);
    Ok(())
}

struct Gate {
    retry_at: Option<Instant>,
    slot: Option<u32>,
    calls: u32,
    clock: Cell<Instant>,
    attempt_takes: Duration,
    deadline: Duration,
}

impl Gate {
    fn new(slot: Option<u32>) -> Self {
        Self {
            retry_at: None,
            slot,
            calls: 0,
            clock: Cell::new(Instant::now()),
            attempt_takes: Duration::ZERO,
            deadline: RESTORE_ATTEMPT_DEADLINE,
        }
    }

    async fn run_mode(&mut self, mode: RestoreMode, now: Instant, fail: bool) -> bool {
        self.clock.set(now);
        let calls = &mut self.calls;
        let clock = &self.clock;
        let takes = self.attempt_takes;
        guarded_restore(
            "conn",
            mode,
            || clock.get(),
            self.deadline,
            &mut self.retry_at,
            &mut self.slot,
            async || {
                *calls += 1;
                clock.set(clock.get() + takes);
                if fail { Err("denied") } else { Ok(9) }
            },
        )
        .await
    }

    async fn run(&mut self, configured: bool, now: Instant, fail: bool) -> bool {
        self.run_mode(RestoreMode::of(configured, false), now, fail)
            .await
    }
}

#[tokio::test]
async fn missing_credentials_of_a_configured_connection_are_restored() {
    let mut gate = Gate::new(None);
    assert!(gate.run(true, Instant::now(), false).await);
    assert_eq!(gate.calls, 1);
    assert_eq!(gate.slot, Some(9));
}

#[tokio::test]
async fn a_removed_connection_is_never_restored() {
    let mut gate = Gate::new(None);
    assert!(gate.run(false, Instant::now(), false).await);
    assert_eq!(gate.calls, 0);
    assert!(gate.slot.is_none());
}

#[tokio::test]
async fn a_removed_connection_pending_retirement_restores_for_retirement_only() {
    assert_eq!(RestoreMode::of(false, true), RestoreMode::RetirementOnly);
    assert_eq!(RestoreMode::of(false, false), RestoreMode::Refused);
    assert_eq!(RestoreMode::of(true, true), RestoreMode::Polling);
    let mut gate = Gate::new(None);
    assert!(
        gate.run_mode(RestoreMode::RetirementOnly, Instant::now(), false)
            .await
    );
    assert_eq!(gate.calls, 1);
    assert_eq!(gate.slot, Some(9));
}

#[tokio::test]
async fn a_removed_connection_not_pending_retirement_is_not_restored() {
    let mut gate = Gate::new(None);
    assert!(
        gate.run_mode(RestoreMode::of(false, false), Instant::now(), false)
            .await
    );
    assert_eq!(gate.calls, 0);
    assert!(gate.slot.is_none());
}

#[tokio::test]
async fn a_restore_error_leaves_the_poll_held() {
    let mut gate = Gate::new(None);
    assert!(!gate.run(true, Instant::now(), true).await);
    assert_eq!(gate.calls, 1);
    assert!(gate.slot.is_none());
}

#[tokio::test]
async fn present_credentials_skip_the_restore() {
    let mut gate = Gate::new(Some(1));
    assert!(gate.run(true, Instant::now(), true).await);
    assert_eq!(gate.calls, 0);
}

#[tokio::test]
async fn a_failed_restore_is_not_retried_until_the_backoff_passes() {
    let start = Instant::now();
    let mut gate = Gate::new(None);
    assert!(!gate.run(true, start, true).await);
    assert!(!gate.run(true, start + RESTORE_RETRY / 2, false).await);
    assert_eq!(gate.calls, 1);
    assert!(gate.run(true, start + RESTORE_RETRY, false).await);
    assert_eq!(gate.calls, 2);
    assert!(gate.retry_at.is_none());
}

#[tokio::test]
async fn the_backoff_starts_after_a_slow_attempt_fails() {
    let start = Instant::now();
    let mut gate = Gate::new(None);
    gate.attempt_takes = Duration::from_secs(40);
    gate.deadline = Duration::from_secs(60);
    assert!(!gate.run(true, start, true).await);
    assert_eq!(
        gate.retry_at,
        Some(start + Duration::from_secs(40) + RESTORE_RETRY)
    );
    let during = start + Duration::from_secs(40) + RESTORE_RETRY / 2;
    assert!(!gate.run(true, during, false).await);
    assert_eq!(gate.calls, 1);
}

#[tokio::test]
async fn a_hanging_restore_returns_within_the_deadline_and_backs_off() {
    let start = Instant::now();
    let mut slot: Option<u32> = None;
    let mut retry_at = None;
    let began = Instant::now();
    let held = !guarded_restore(
        "conn",
        RestoreMode::Polling,
        || start,
        Duration::from_millis(50),
        &mut retry_at,
        &mut slot,
        async || -> Result<u32, &str> { std::future::pending().await },
    )
    .await;
    assert!(held);
    assert!(began.elapsed() < Duration::from_secs(5));
    assert!(slot.is_none());
    assert_eq!(retry_at, Some(start + RESTORE_RETRY));
}

async fn held_poll(gate: &mut Gate, error: &HostError, now: Instant) -> bool {
    if !gate.run(true, now, false).await {
        return false;
    }
    release_suspect_credentials(&mut gate.slot, error);
    true
}

#[tokio::test]
async fn a_local_hold_keeps_the_credentials_so_the_next_poll_restores_nothing() {
    let mut gate = Gate::new(Some(1));
    let now = Instant::now();
    for error in [HostError::Config, HostError::poll(&EnsureError::Uncertain)] {
        assert!(held_poll(&mut gate, &error, now).await);
        assert_eq!(gate.slot, Some(1));
    }
    assert_eq!(gate.calls, 0);
}

#[tokio::test]
async fn an_authorization_failure_drops_the_credentials_and_the_next_poll_restores_once() {
    for cause in [
        EnsureError::Forbidden,
        EnsureError::Rejected,
        EnsureError::Conflict,
        EnsureError::Unauthorized,
    ] {
        let mut gate = Gate::new(Some(1));
        let start = Instant::now();
        assert!(held_poll(&mut gate, &HostError::poll(&cause), start).await);
        assert!(gate.slot.is_none());
        assert!(gate.run(true, start, false).await);
        assert_eq!(gate.calls, 1);
        assert_eq!(gate.slot, Some(9));
        assert!(gate.run(true, start, false).await);
        assert_eq!(gate.calls, 1);
    }
}

#[tokio::test]
async fn a_transport_uncertainty_keeps_the_credentials_and_restores_nothing() {
    let mut gate = Gate::new(Some(1));
    let now = Instant::now();
    let error = HostError::poll(&EnsureError::Uncertain);
    assert!(held_poll(&mut gate, &error, now).await);
    assert_eq!(gate.slot, Some(1));
    assert!(gate.run(true, now, false).await);
    assert_eq!(gate.calls, 0);
}

#[tokio::test]
async fn repeated_local_holds_never_reopen_a_session() {
    let mut gate = Gate::new(Some(1));
    let start = Instant::now();
    for tick in 0..2000_u32 {
        let now = start + Duration::from_secs(2 * u64::from(tick));
        assert!(held_poll(&mut gate, &HostError::Keychain, now).await);
    }
    assert_eq!(gate.calls, 0);
}
