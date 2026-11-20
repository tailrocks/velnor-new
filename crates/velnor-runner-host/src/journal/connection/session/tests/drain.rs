// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use tempfile::TempDir;

use crate::config::HostConfig;
use crate::journal::{CapacityLimits, ConnectionJournal, Journal, OperationalCapacityLimits};
use crate::launch::CompletedHeaderPoll;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const NONCE: &str = "0123456789abcdef0123456789abcdef";
const OTHER_NONCE: &str = "fedcba9876543210fedcba9876543210";
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
[[connections]]
connection_id = "conn-test-2"
repository_id = 43
repository = "owner/other"
credential_ref = "keychain:velnor.test/conn-test-2"
[connections.scale_set]
name = "ubuntu-24.04-scale-set"
labels = ["velnor", "ubuntu-24.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 2
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

struct Fixture {
    _dir: TempDir,
    journal: Journal,
    config: HostConfig,
    scope: ConnectionJournal,
    other: ConnectionJournal,
}

async fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = HostConfig::parse(CONFIG)?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    journal.bind_engine("engine-test").await?;
    let bindings = config.connections();
    let scope = journal
        .connection_scope(bindings.first().ok_or("connection 1")?)
        .await?;
    let other = journal
        .connection_scope(bindings.get(1).ok_or("connection 2")?)
        .await?;
    let connection = journal.connection().await?;
    for (id, set) in [("conn-test-1", 7), ("conn-test-2", 8)] {
        connection
            .execute(
                "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES (?1, ?2, 'ubuntu-26.04-scale-set', 'created', 'engine-test', 'unix:///docker.sock', 'linux/amd64')",
                (id, set),
            )
            .await?;
    }
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 2,
                cpu_millicores: 6_000,
                memory_bytes: 13 << 30,
            },
            OperationalCapacityLimits {
                cpu_millicores: 500,
                memory_bytes: 1 << 30,
            },
        )
        .await?;
    Ok(Fixture {
        _dir: dir,
        journal,
        config,
        scope,
        other,
    })
}

async fn bind_session(
    scope: &ConnectionJournal,
    nonce: &str,
    session: &str,
    target: u32,
) -> Result<i64, Box<dyn std::error::Error>> {
    scope.prepare_session_create(nonce).await?;
    let claim = scope
        .claim_session_create(nonce)
        .await?
        .ok_or("session claim")?;
    Ok(scope
        .record_created_session(&claim, session, target)
        .await?
        .intent_id())
}

async fn run(journal: &Journal, sql: &str) -> TestResult {
    journal.connection().await?.execute(sql, ()).await?;
    Ok(())
}

const LAUNCH: &str = "INSERT INTO connection_intents (connection_id, kind, subject, state, scale_set_id) VALUES ('conn-test-1', 'launch', 'runner-1', 'pending', 7)";

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_with_only_its_own_bound_session_is_drained() -> TestResult {
    let fx = fixture().await?;
    assert!(fx.scope.is_drained().await?);
    bind_session(&fx.scope, NONCE, "session-1", 0).await?;
    assert!(fx.scope.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_charged_session_header_does_not_block_the_connection_drain() -> TestResult {
    let fx = fixture().await?;
    let intent = bind_session(&fx.scope, NONCE, "session-1", 1).await?;
    let allocation = fx.scope.session_header_allocation(intent).await?;
    assert_eq!(allocation.target(), 1);
    assert!(fx.scope.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unsettled_header_poll_keeps_the_session_in_the_drain_count() -> TestResult {
    let fx = fixture().await?;
    let intent = bind_session(&fx.scope, NONCE, "session-1", 1).await?;
    let allocation = fx.scope.session_header_allocation(intent).await?;
    let attempt = fx.scope.begin_zero_header_poll(intent, &allocation).await?;
    assert!(!fx.scope.is_drained().await?);
    assert!(!fx.journal.is_drained().await?);
    fx.scope
        .mark_session_poll_uncertain(intent, attempt)
        .await?;
    assert!(!fx.scope.is_drained().await?);
    assert!(!fx.journal.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unsettled_session_delete_keeps_the_session_in_the_drain_count() -> TestResult {
    let fx = fixture().await?;
    let intent = bind_session(&fx.scope, NONCE, "session-1", 1).await?;
    let allocation = fx.scope.session_header_allocation(intent).await?;
    let poll = fx.scope.begin_zero_header_poll(intent, &allocation).await?;
    fx.scope
        .settle_session_poll(intent, CompletedHeaderPoll::for_test(poll))
        .await?;
    assert!(fx.scope.is_drained().await?);
    let attempt = fx.scope.begin_session_delete(intent, &allocation).await?;
    assert!(!fx.scope.is_drained().await?);
    assert!(!fx.journal.is_drained().await?);
    fx.scope
        .mark_session_delete_uncertain(intent, attempt)
        .await?;
    assert!(!fx.scope.is_drained().await?);
    assert!(!fx.journal.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pending_scale_set_delete_keeps_the_host_from_draining() -> TestResult {
    let fx = fixture().await?;
    bind_session(&fx.scope, NONCE, "session-1", 0).await?;
    assert!(fx.journal.is_drained().await?);
    for state in ["requested", "uncertain"] {
        run(
            &fx.journal,
            &format!("INSERT INTO connection_scale_set_deletes (connection_id, scale_set_id, runner_group_id, state) VALUES ('conn-test-1', 7, 1, '{state}')"),
        )
        .await?;
        assert!(!fx.journal.is_drained().await?, "{state}");
        run(&fx.journal, "DELETE FROM connection_scale_set_deletes").await?;
    }
    run(
        &fx.journal,
        "INSERT INTO connection_scale_set_deletes (connection_id, scale_set_id, runner_group_id, state) VALUES ('conn-test-1', 7, 1, 'deleted')",
    )
    .await?;
    assert!(fx.journal.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pending_scale_set_delete_blocks_reactivation() -> TestResult {
    let fx = fixture().await?;
    let binding = fx.config.connections().first().ok_or("connection")?;
    run(
        &fx.journal,
        "UPDATE connection_scopes SET retired = 1 WHERE connection_id = 'conn-test-1'",
    )
    .await?;
    for state in ["requested", "uncertain"] {
        run(
            &fx.journal,
            &format!("INSERT INTO connection_scale_set_deletes (connection_id, scale_set_id, runner_group_id, state) VALUES ('conn-test-1', 7, 1, '{state}')"),
        )
        .await?;
        assert!(!fx.journal.reactivate_if_drained(binding).await?, "{state}");
        run(&fx.journal, "DELETE FROM connection_scale_set_deletes").await?;
    }
    assert!(fx.journal.reactivate_if_drained(binding).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bound_session_plus_a_running_job_is_not_drained() -> TestResult {
    let fx = fixture().await?;
    bind_session(&fx.scope, NONCE, "session-1", 0).await?;
    run(&fx.journal, LAUNCH).await?;
    assert!(!fx.scope.is_drained().await?);
    run(
        &fx.journal,
        "UPDATE connection_intents SET cleanup_proven = 1 WHERE kind = 'launch'",
    )
    .await?;
    assert!(fx.scope.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bound_session_plus_another_session_create_is_not_drained() -> TestResult {
    let fx = fixture().await?;
    bind_session(&fx.scope, NONCE, "session-1", 0).await?;
    run(
        &fx.journal,
        "INSERT INTO connection_session_creates (connection_id, scale_set_id, local_nonce, state) VALUES ('conn-test-1', 9, 'fedcba9876543210fedcba9876543210', 'prepared')",
    )
    .await?;
    assert!(!fx.scope.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bound_session_plus_a_scale_set_create_in_flight_is_not_drained() -> TestResult {
    let fx = fixture().await?;
    bind_session(&fx.scope, NONCE, "session-1", 0).await?;
    run(
        &fx.journal,
        &format!("INSERT INTO connection_scale_set_intents (connection_id, binding_digest, repository_id, repository, selector_name, selector_labels, runner_group_id, state) VALUES ('conn-test-1', '{}', 42, 'owner/repo', 'ubuntu-26.04-scale-set', '[\"velnor\"]', 1, 'requested')", "a".repeat(64)),
    )
    .await?;
    assert!(!fx.scope.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ambiguous_legacy_work_blocks_a_connection_with_a_bound_session() -> TestResult {
    let fx = fixture().await?;
    bind_session(&fx.scope, NONCE, "session-1", 0).await?;
    run(
        &fx.journal,
        "INSERT INTO intents (kind, subject, state, cleanup_proven) VALUES ('launch', 'legacy', 'pending', 0)",
    )
    .await?;
    assert!(!fx.scope.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn is_retired_and_drained_stays_strict_while_the_session_is_bound() -> TestResult {
    let fx = fixture().await?;
    bind_session(&fx.scope, NONCE, "session-1", 0).await?;
    let binding = fx.config.connections().first().ok_or("connection")?;
    assert!(!fx.journal.retire_connection_if_drained(binding).await?);
    run(
        &fx.journal,
        "UPDATE connection_scopes SET retired = 1 WHERE connection_id = 'conn-test-1'",
    )
    .await?;
    assert!(!fx.journal.is_retired_and_drained(binding).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn host_wide_drain_ignores_bound_sessions_but_not_other_work() -> TestResult {
    let fx = fixture().await?;
    assert!(fx.journal.is_drained().await?);
    bind_session(&fx.scope, NONCE, "session-1", 1).await?;
    bind_session(&fx.other, OTHER_NONCE, "session-2", 0).await?;
    assert!(fx.journal.is_drained().await?);
    run(&fx.journal, LAUNCH).await?;
    assert!(!fx.journal.is_drained().await?);
    run(
        &fx.journal,
        "UPDATE connection_intents SET cleanup_proven = 1 WHERE kind = 'launch'",
    )
    .await?;
    assert!(fx.journal.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn one_connections_work_does_not_drain_or_block_another_scope() -> TestResult {
    let fx = fixture().await?;
    bind_session(&fx.scope, NONCE, "session-1", 0).await?;
    bind_session(&fx.other, OTHER_NONCE, "session-2", 0).await?;
    run(
        &fx.journal,
        "INSERT INTO connection_intents (connection_id, kind, subject, state, scale_set_id) VALUES ('conn-test-2', 'launch', 'runner-2', 'pending', 8)",
    )
    .await?;
    assert!(fx.scope.is_drained().await?);
    assert!(!fx.other.is_drained().await?);
    assert!(!fx.journal.is_drained().await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn host_wide_drain_counts_an_unresolved_refresh_of_a_bound_session() -> TestResult {
    let fx = fixture().await?;
    let intent = bind_session(
        &fx.scope,
        "0123456789abcdef0123456789abcdef",
        "session-1",
        1,
    )
    .await?;
    fx.scope.session_header_allocation(intent).await?;
    assert!(fx.journal.is_drained().await?);
    let claim = fx
        .scope
        .claim_session_refresh(intent, "session-1")
        .await?
        .ok_or("no refresh claim")?;
    assert!(!fx.journal.is_drained().await?);
    fx.scope.mark_session_refresh_uncertain(claim).await?;
    assert!(!fx.journal.is_drained().await?);
    run(
        &fx.journal,
        "UPDATE connection_session_refreshes SET state = 'succeeded'",
    )
    .await?;
    assert!(fx.journal.is_drained().await?);
    Ok(())
}
