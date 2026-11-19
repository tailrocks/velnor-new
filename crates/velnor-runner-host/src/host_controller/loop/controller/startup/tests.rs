// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{HOLD_REPEAT_TICKS, HoldLog, admission_after, legacy_admission};
use crate::config::HostConfig;
use crate::journal::Journal;
use crate::reconcile::Reconcile;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const HOLD: Reconcile = Reconcile::Hold {
    adopt: Vec::new(),
    occupied: 1,
};

type Fixture = (
    tempfile::TempDir,
    Journal,
    bollard::Docker,
    std::os::unix::net::UnixListener,
);

async fn fixture() -> TestResult<Fixture> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket)?;
    // Rows seeded here name no container and no runner, so the gate never calls Docker or GitHub.
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    Ok((dir, journal, docker, listener))
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

fn config() -> TestResult<HostConfig> {
    Ok(HostConfig::parse(CONFIG)?)
}

#[test]
fn admission_needs_storage_and_an_advertising_gate() {
    assert!(!admission_after(true, &HOLD));
    assert!(!admission_after(
        false,
        &Reconcile::Advertise { occupied: 0 }
    ));
    assert!(admission_after(true, &Reconcile::Advertise { occupied: 0 }));
}

#[tokio::test]
async fn a_pending_legacy_row_keeps_admission_closed_across_retries() -> TestResult {
    let (_dir, journal, docker, _socket) = fixture().await?;
    journal.begin_launch("legacy-subject").await?;
    let mut log = HoldLog::default();
    for _ in 0..3 {
        assert!(!legacy_admission(&journal, &docker, &config()?, &mut log).await);
    }
    Ok(())
}

#[tokio::test]
async fn a_cleanup_proof_lets_the_next_retry_open_admission() -> TestResult {
    let (_dir, journal, docker, _socket) = fixture().await?;
    let (id, _) = journal.begin_launch("legacy-subject").await?;
    let mut log = HoldLog::default();
    assert!(!legacy_admission(&journal, &docker, &config()?, &mut log).await);
    journal.record_cleanup(id).await?;
    assert!(legacy_admission(&journal, &docker, &config()?, &mut log).await);
    Ok(())
}

#[tokio::test]
async fn a_journal_without_blocking_rows_opens_admission() -> TestResult {
    let (_dir, journal, docker, _socket) = fixture().await?;
    let mut log = HoldLog::default();
    assert!(legacy_admission(&journal, &docker, &config()?, &mut log).await);
    Ok(())
}

#[test]
fn hold_lines_repeat_only_on_change_or_every_tenth_tick() {
    let mut log = HoldLog::default();
    let lines = vec!["legacy_hold row=1".to_owned()];
    assert!(log.on_hold(&lines));
    for _ in 1..HOLD_REPEAT_TICKS {
        assert!(!log.on_hold(&lines));
    }
    assert!(log.on_hold(&lines));
    assert!(log.on_hold(&["legacy_hold row=2".to_owned()]));
    assert!(log.on_open());
    assert!(!log.on_open());
}
