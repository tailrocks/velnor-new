// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::Journal;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[path = "tests/other_container.rs"]
mod other_container;

const RESERVATION: i64 = 2;
const CONNECTION: &str = "conn-test-1";
const ENGINE: &str = "engine-test";
const ROOT: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const SNAPSHOT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EVIDENCE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const LEASE_UNTIL: i64 = 4_102_444_800;

const SHAPE: &str = "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES ('conn-test-1', 7, 'ubuntu-26.04-scale-set', 'created', 'engine-test', 'unix:///docker.sock', 'linux/amd64');
INSERT INTO host_storage_profiles (generation_id, profile_digest) VALUES ('bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
INSERT INTO connection_intents (id, connection_id, kind, subject, state, scale_set_id, docker_engine_id, launch_phase, assignment_key) VALUES (1, 'conn-test-1', 'launch', 'req-1', 'uncertain', 7, 'engine-test', 'provisioning_held', 'assign-1');
INSERT INTO capacity_reservations (id, intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state) VALUES (2, 1, 'conn-test-1', 'assign-1', 'engine-test', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 3000, 6442450944, 'uncertain');
INSERT INTO capacity_reservation_provisioning (reservation_id, operation_id, profile_generation_id, profile_digest, sample_generation_id, observed_at_unix_ms, engine_id, docker_root_digest, retained_memory_bytes) VALUES (2, 1, 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 1700000000000, 'engine-test', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 0);
INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state) VALUES (2, 'not_requested');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES (2, 'shared-run', 'w2', 'w2', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES (2, 'runner-home', 'w2-home', 'w2-home', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES (2, 'runner-work', 'w2-work', 'w2-work', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES (2, 'dind-data', 'w2-docker', 'w2-docker', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, publication_policy_digest, create_state) VALUES (2, 'engine-metadata', 'w2-metadata', 'w2-metadata', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'created');
INSERT INTO capacity_reservation_networks (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, network_name, labels_json, create_state, network_id) VALUES (2, 'conn-test-1', 1, 'engine-test', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 'velnor-w2-private', '{\"io.velnor.connection-id\":\"conn-test-1\",\"io.velnor.engine-id\":\"engine-test\",\"io.velnor.managed\":\"true\",\"io.velnor.operation-id\":\"1\",\"io.velnor.reservation-id\":\"2\"}', 'created', 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff');
INSERT INTO capacity_reservation_storage_initialization (reservation_id, connection_id, operation_id, generation, engine_id, docker_root_digest, image_runtime_id, image_source_sha, nano_cpus, memory_bytes, pids_limit, home_volume, work_volume, layout_version, metadata_volume, metadata_volume_id, helper_name, helper_id, home_device_major, home_device_minor, home_inode, home_mount_id, work_device_major, work_device_minor, work_inode, work_mount_id, create_dispatch_state, create_state, start_state, result_state, remove_state, abort_state, operation_key) VALUES (2, 'conn-test-1', 1, 1, 'engine-test', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 'sha256:1111111111111111111111111111111111111111111111111111111111111111', '1111111111111111111111111111111111111111', 500000000, 67108864, 16, 'w2-home', 'w2-work', 2, 'w2-metadata', 'w2-metadata', 'velnor-init-1-1', 'c111111111111111111111111111111111111111111111111111111111111111', 1, 2, 3, 4, 5, 6, 7, 8, 'response_received', 'created', 'started', 'confirmed', 'not_requested', 'not_requested', '1-1')";

fn container_row(role: &str, name: &str, create_state: &str, id: Option<&str>) -> String {
    let id = match id {
        Some(value) => format!("'{value}'"),
        None => "NULL".to_owned(),
    };
    format!(
        "INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, container_id, create_state, start_state) VALUES ({RESERVATION}, '{role}', '{name}', {id}, '{create_state}', 'not_requested')"
    )
}

fn claim_and_progress(phase: &str) -> String {
    format!(
        "INSERT INTO capacity_reservation_pre_jit_cleanup_claims (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, snapshot_sha256, claim_generation, lease_until, phase) VALUES ({RESERVATION}, '{CONNECTION}', 1, '{ENGINE}', '{ROOT}', '{SNAPSHOT}', 1, {LEASE_UNTIL}, 'claimed');\nINSERT INTO capacity_reservation_pre_jit_cleanup_progress (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, snapshot_sha256, claim_generation, phase, evidence_sha256) VALUES ({RESERVATION}, '{CONNECTION}', 1, '{ENGINE}', '{ROOT}', '{SNAPSHOT}', 1, '{phase}', '{SNAPSHOT}')"
    )
}

fn binding() -> TestResult<RepositoryConnection> {
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse(CONNECTION)?,
        42,
        "owner/repo".to_owned(),
        KeychainReference::parse("keychain:velnor.test/conn-test-1")?,
        ScaleSetSelector::try_new(
            "ubuntu-26.04-scale-set",
            &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
        )?,
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            vec!["main".to_owned()],
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )?,
    )?)
}

async fn seed(journal: &Journal, statements: &str) -> TestResult<()> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' ORDER BY rowid",
            (),
        )
        .await?;
    let mut saved = Vec::new();
    while let Some(row) = rows.next().await? {
        saved.push((row.get::<String>(0)?, row.get::<String>(1)?));
    }
    for (name, _) in &saved {
        connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await?;
    }
    for statement in statements.split('\n') {
        connection.execute(statement, ()).await?;
    }
    for (_, sql) in &saved {
        connection.execute(sql, ()).await?;
    }
    Ok(())
}

async fn scenario(
    runner: (&str, Option<&str>),
    dind: (&str, Option<&str>),
    corrupted: &[&str],
) -> TestResult<(tempfile::TempDir, Journal)> {
    let state = tempfile::tempdir()?;
    let journal = Journal::open(&state.path().join("launch.db")).await?;
    journal.bind_engine(ENGINE).await?;
    journal.connection_scope(&binding()?).await?;
    let mut statements = format!("{SHAPE}\n");
    statements.push_str(&container_row("runner", "w2-runner", runner.0, runner.1));
    statements.push('\n');
    statements.push_str(&container_row("dind", "w2-dind", dind.0, dind.1));
    statements.push('\n');
    for statement in corrupted {
        statements.push_str(statement);
        statements.push('\n');
    }
    statements.push_str(&claim_and_progress("claimed"));
    seed(&journal, &statements).await?;
    Ok((state, journal))
}

async fn advance(
    journal: &Journal,
    from: &str,
    to: &str,
) -> Result<Result<(), turso::Error>, Box<dyn std::error::Error>> {
    let outcome = journal
        .connection()
        .await?
        .execute(
            &format!(
                "UPDATE capacity_reservation_pre_jit_cleanup_progress SET phase = '{to}', evidence_sha256 = '{EVIDENCE}' WHERE reservation_id = {RESERVATION} AND phase = '{from}'"
            ),
            (),
        )
        .await
        .map(|_| ());
    Ok(outcome)
}

#[tokio::test]
async fn a_pre_jit_progress_update_with_both_bootstrap_containers_created_is_refused()
-> TestResult<()> {
    let (_state, journal) = scenario(
        ("created", Some("still-here")),
        ("created", Some("still-dind")),
        &[],
    )
    .await?;
    let outcome = advance(&journal, "claimed", "bootstrap_absent").await?;
    let error = outcome.expect_err(
        "progress advanced to bootstrap_absent while both bootstrap containers are created",
    );
    assert!(
        error
            .to_string()
            .contains("invalid pre-JIT cleanup progress update"),
        "refusal came from the progress guard, not another constraint: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn a_previous_guard_journal_migrates_to_the_current_guard_on_open() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("launch.db");
    {
        let journal = Journal::open(&path).await?;
        let connection = journal.connection().await?;
        crate::journal::schema::post_jit_effect_fences::downgrade_fences_to_v36(&connection)
            .await?;
        connection.execute("PRAGMA user_version = 53", ()).await?;
        connection
            .execute("DROP TRIGGER pre_jit_cleanup_progress_update_guard", ())
            .await?;
        connection
            .execute(super::PROGRESS_UPDATE_GUARD_V53, ())
            .await?;
    }
    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'pre_jit_cleanup_progress_update_guard'",
            (),
        )
        .await?;
    let sql: String = rows
        .next()
        .await?
        .ok_or("trigger missing after migration")?
        .get(0)?;
    assert_eq!(
        super::super::sql_normalization::normalize(&sql),
        super::super::sql_normalization::normalize(super::PROGRESS_UPDATE_GUARD)
    );
    Ok(())
}

#[tokio::test]
async fn a_current_journal_with_the_previous_guard_is_refused_at_open() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("launch.db");
    {
        let journal = Journal::open(&path).await?;
        let connection = journal.connection().await?;
        connection
            .execute("DROP TRIGGER pre_jit_cleanup_progress_update_guard", ())
            .await?;
        connection
            .execute(super::PROGRESS_UPDATE_GUARD_V53, ())
            .await?;
    }
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn an_altered_version_53_guard_is_refused_before_it_is_replaced() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("launch.db");
    {
        let journal = Journal::open(&path).await?;
        let connection = journal.connection().await?;
        connection.execute("PRAGMA user_version = 53", ()).await?;
        connection
            .execute("DROP TRIGGER pre_jit_cleanup_progress_update_guard", ())
            .await?;
        connection
            .execute(
                "CREATE TRIGGER pre_jit_cleanup_progress_update_guard BEFORE UPDATE ON capacity_reservation_pre_jit_cleanup_progress BEGIN SELECT 1; END",
                (),
            )
            .await?;
    }
    assert!(
        Journal::open(&path).await.is_err(),
        "a tampered v53 guard must be refused, not silently replaced by the v54 guard"
    );
    Ok(())
}

#[tokio::test]
async fn the_ladder_walks_when_the_claim_premise_is_intact() -> TestResult<()> {
    let (_state, journal) = scenario(
        ("uncertain", None),
        ("created", Some("gone-from-engine")),
        &[],
    )
    .await?;
    for (from, to) in [
        ("claimed", "bootstrap_absent"),
        ("bootstrap_absent", "initializer_absent"),
        ("initializer_absent", "storage_absent"),
        ("storage_absent", "network_absent"),
        ("network_absent", "settled"),
    ] {
        let outcome = advance(&journal, from, to).await?;
        assert!(
            outcome.is_ok(),
            "ladder step {from} -> {to} refused on an intact claim premise: {outcome:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn an_initializer_moved_toward_removal_refuses_initializer_absent() -> TestResult<()> {
    let (_state, journal) = scenario(
        ("uncertain", None),
        ("uncertain", None),
        &["UPDATE capacity_reservation_storage_initialization SET remove_state = 'removed' WHERE reservation_id = 2"],
    )
    .await?;
    assert!(
        advance(&journal, "claimed", "bootstrap_absent")
            .await?
            .is_ok()
    );
    assert!(
        advance(&journal, "bootstrap_absent", "initializer_absent")
            .await?
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn a_missing_created_storage_object_refuses_storage_absent() -> TestResult<()> {
    let (_state, journal) = scenario(
        ("uncertain", None),
        ("uncertain", None),
        &["UPDATE capacity_reservation_provisioning_objects SET create_state = 'unclaimed', docker_id = NULL WHERE reservation_id = 2 AND role = 'dind-data'"],
    )
    .await?;
    assert!(
        advance(&journal, "claimed", "bootstrap_absent")
            .await?
            .is_ok()
    );
    assert!(
        advance(&journal, "bootstrap_absent", "initializer_absent")
            .await?
            .is_ok()
    );
    assert!(
        advance(&journal, "initializer_absent", "storage_absent")
            .await?
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn a_network_moved_toward_removal_refuses_network_absent() -> TestResult<()> {
    let (_state, journal) = scenario(
        ("uncertain", None),
        ("uncertain", None),
        &["UPDATE capacity_reservation_networks SET remove_state = 'removed' WHERE reservation_id = 2"],
    )
    .await?;
    assert!(
        advance(&journal, "claimed", "bootstrap_absent")
            .await?
            .is_ok()
    );
    assert!(
        advance(&journal, "bootstrap_absent", "initializer_absent")
            .await?
            .is_ok()
    );
    assert!(
        advance(&journal, "initializer_absent", "storage_absent")
            .await?
            .is_ok()
    );
    assert!(
        advance(&journal, "storage_absent", "network_absent")
            .await?
            .is_err()
    );
    Ok(())
}
