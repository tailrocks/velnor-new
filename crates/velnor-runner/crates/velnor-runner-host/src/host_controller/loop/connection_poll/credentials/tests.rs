// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use super::{RESTORE_RETRY, guarded_restore, restore_bound_session, restore_missing};
use crate::{
    config::HostConfig,
    error::HostError,
    journal::{ConnectionJournal, Journal},
};

struct Connection {
    credentials: Option<u32>,
    restores: u32,
    recoveries: u32,
}

impl Connection {
    async fn poll(&mut self, fail: bool) -> Result<(), ()> {
        let restores = &mut self.restores;
        restore_missing(&mut self.credentials, async || {
            *restores += 1;
            Ok::<_, ()>(7)
        })
        .await?;
        if fail {
            self.credentials.take();
            return Err(());
        }
        self.recoveries += 1;
        Ok(())
    }
}

#[tokio::test]
async fn a_held_poll_does_not_strand_the_next_poll() {
    let mut connection = Connection {
        credentials: Some(7),
        restores: 0,
        recoveries: 0,
    };
    assert!(connection.poll(true).await.is_err());
    assert!(connection.credentials.is_none());
    assert!(connection.poll(false).await.is_ok());
    assert_eq!(connection.restores, 1);
    assert_eq!(connection.recoveries, 1);
    assert!(connection.credentials.is_some());
}

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
}

impl Gate {
    fn new(slot: Option<u32>) -> Self {
        Self {
            retry_at: None,
            slot,
            calls: 0,
        }
    }

    async fn run(&mut self, configured: bool, now: Instant, fail: bool) -> bool {
        let calls = &mut self.calls;
        guarded_restore(
            "conn",
            configured,
            now,
            &mut self.retry_at,
            &mut self.slot,
            async || {
                *calls += 1;
                if fail { Err("denied") } else { Ok(9) }
            },
        )
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
