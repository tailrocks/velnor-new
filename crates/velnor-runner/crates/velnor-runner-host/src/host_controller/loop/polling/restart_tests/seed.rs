// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use sha2::{Digest, Sha256};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::{ConnectionJournal, Journal};
use crate::launch::SelectedEngineContext;

use super::TestResult;

pub(super) const CONNECTION_ID: &str = "conn-test-1";
pub(super) const ENGINE_ID: &str = "engine-test";
pub(super) const PRIVATE_VOLUME: &str = "w0123456789abcdef0123456789abcdef";
pub(super) const IMAGE_ID: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

const BINDING: &str = "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES ('conn-test-1', 7, 'ubuntu-26.04-scale-set', 'created', 'engine-test', 'unix:///docker.sock', 'linux/amd64')";

const RESERVATION: &str = "INSERT INTO connection_intents (id, connection_id, kind, subject, state, scale_set_id, docker_engine_id, launch_phase, assignment_key) VALUES ({i}, 'conn-test-1', 'launch', 'req-{i}', '{intent}', 7, 'engine-test', 'provisioning_held', 'assign-{i}');
INSERT INTO capacity_reservations (id, intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state, remote_cleanup_proven) VALUES ({r}, {i}, 'conn-test-1', 'assign-{i}', 'engine-test', '{root}', '{binding}', '{hex32}', 3000, 6442450944, 'reserved', {cleanup});
INSERT INTO capacity_reservation_provisioning (reservation_id, operation_id, profile_generation_id, profile_digest, sample_generation_id, observed_at_unix_ms, engine_id, docker_root_digest, retained_memory_bytes) VALUES ({r}, {i}, '{hex32}', '{hex64}', '{hex32}', 1700000000000, 'engine-test', '{root}', 0);
INSERT INTO capacity_reservation_provisioning_topology (reservation_id, topology_version) VALUES ({r}, 2);
INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state{state_columns}) VALUES ({r}, '{bootstrap_start}'{state_values});
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'shared-run', '{volume}', '{volume}', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'runner-home', '{volume}-home', '{volume}-home', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'runner-work', '{volume}-work', '{volume}-work', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'dind-data', '{volume}-docker', '{volume}-docker', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, publication_policy_digest, create_state) VALUES ({r}, 'engine-metadata', '{volume}-metadata', '{volume}-metadata', '{hex64}', 'created')";

const INTERRUPTED: &str = "INSERT INTO capacity_reservation_storage_initialization (reservation_id, connection_id, operation_id, generation, engine_id, docker_root_digest, image_runtime_id, image_source_sha, nano_cpus, memory_bytes, pids_limit, home_volume, work_volume, layout_version, metadata_volume, metadata_volume_id, helper_name, create_dispatch_state, create_state, start_state, result_state, remove_state, abort_state, operation_key) VALUES ({r}, 'conn-test-1', {i}, 1, 'engine-test', '{root}', '{image}', '{hex40}', 500000000, 67108864, 16, '{volume}-home', '{volume}-work', 2, '{volume}-metadata', '{volume}-metadata', 'velnor-init-{i}-1', 'dispatched', 'requested', 'not_requested', 'not_requested', 'not_requested', '{abort}', '{i}-1')";

const NONCE: &str = "0123456789abcdef0123456789abcdef";

const PHYSICAL: &str = "UPDATE capacity_reservations SET assignment_key = 'v1|conn-test-1|7|remote|6:runner|32:{nonce}' WHERE id = {r};
UPDATE connection_intents SET assignment_key = 'v1|conn-test-1|7|remote|6:runner|32:{nonce}', runner_name = 'v{nonce}' WHERE id = {i};
INSERT INTO connection_request_authorizations (connection_id, scale_set_id, request_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json, acquire_state) VALUES ('conn-test-1', 7, {r}, 0, 'branch_restricted', 'created_empty_exclusive_admin_session_v1', '{binding}', '{hex64}', '{}', 'authorized');
INSERT INTO physical_attempt_lineages (lineage_id, connection_id, scale_set_id, binding_digest) VALUES ({r}, 'conn-test-1', 7, '{binding}');
INSERT INTO physical_attempts (lineage_id, generation, reservation_id, intent_id, physical_nonce, connection_id, scale_set_id, witness_request_id, binding_digest, source_digest, engine_id, docker_root_digest, created_at) VALUES ({r}, 1, {r}, {i}, '{nonce}', 'conn-test-1', 7, {r}, '{binding}', '{hex64}', 'engine-test', '{root}', 0);
INSERT INTO physical_attempt_current (lineage_id, generation, reservation_id, intent_id) VALUES ({r}, 1, {r}, {i})";

const ADMITTED_PAIR: &str = "INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, create_state, start_state) VALUES ({r}, 'runner', '{volume}-runner', 'unclaimed', 'not_requested');
INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, create_state, start_state) VALUES ({r}, 'dind', '{volume}-dind', 'unclaimed', 'not_requested')";

const CLAIMED_CONTAINER: &str = "INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, create_state, start_state) VALUES ({r}, 'runner', '{volume}-runner', 'requested', 'not_requested')";

const RECORDED: &str = "SELECT reservation.state, intent.state, init.create_dispatch_state, init.create_state, init.start_state, init.abort_state, init.abort_helper_id FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id JOIN capacity_reservation_storage_initialization AS init ON init.reservation_id = reservation.id WHERE reservation.id = ?1";

#[derive(Clone, Copy)]
struct Shape<'a> {
    intent: &'a str,
    cleanup: &'a str,
    bootstrap_start: &'a str,
    state: (&'a str, &'a str),
    abort: &'a str,
    claimed_container: bool,
}

const CRASHED: Shape<'static> = Shape {
    intent: "pending",
    cleanup: "0",
    bootstrap_start: "not_requested",
    state: ("", ""),
    abort: "not_requested",
    claimed_container: false,
};

const LIVE: [Shape<'static>; 7] = [
    Shape {
        claimed_container: true,
        ..CRASHED
    },
    Shape {
        bootstrap_start: "requested",
        ..CRASHED
    },
    Shape {
        state: (
            ", resources_bound, instance_binding_digest, guest_boot_id",
            ", 1, '{hex64}', 'boot'",
        ),
        ..CRASHED
    },
    Shape {
        state: (", jit_state", ", 'requested'"),
        ..CRASHED
    },
    Shape {
        cleanup: "1",
        ..CRASHED
    },
    Shape {
        intent: "done",
        ..CRASHED
    },
    Shape {
        abort: "uncertain",
        ..CRASHED
    },
];

pub(super) const LIVE_RESERVATIONS: std::ops::Range<i64> = 3..10;

fn binding() -> TestResult<RepositoryConnection> {
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse(CONNECTION_ID)?,
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

pub(super) async fn reservation_at_storage_initialization(
    journal: &Journal,
    docker: &Docker,
) -> TestResult<(ConnectionJournal, String)> {
    journal.bind_engine(ENGINE_ID).await?;
    let binding = binding()?;
    let scope = journal.connection_scope(&binding).await?;
    let binding_digest: String = Sha256::digest(binding.canonical_identity_bytes()?)
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect();
    execute(journal, BINDING).await?;
    let selected = scope.engine_context().await?.ok_or("engine context")?;
    let root = SelectedEngineContext::capture(&selected, &docker.info().await?)?
        .docker_root_digest()
        .to_owned();
    let (hex32, hex64) = ("b".repeat(32), "a".repeat(64));
    let profile = "INSERT INTO host_storage_profiles (generation_id, profile_digest) VALUES ('{hex32}', '{hex64}')";
    let fill = |template: &str, reservation: i64, volume: &str, shape: Shape<'_>| {
        template
            .replace("{state_columns}", shape.state.0)
            .replace("{state_values}", shape.state.1)
            .replace("{bootstrap_start}", shape.bootstrap_start)
            .replace("{intent}", shape.intent)
            .replace("{cleanup}", shape.cleanup)
            .replace("{abort}", shape.abort)
            .replace("{hex32}", &hex32)
            .replace("{hex40}", &"1".repeat(40))
            .replace("{hex64}", &hex64)
            .replace("{root}", &root)
            .replace("{image}", IMAGE_ID)
            .replace("{binding}", &binding_digest)
            .replace("{volume}", volume)
            .replace("{r}", &reservation.to_string())
            .replace("{i}", &(reservation - 1).to_string())
    };
    execute(journal, &fill(profile, 0, "", CRASHED)).await?;
    let admitted = [RESERVATION, ADMITTED_PAIR].join(";\n");
    execute_all(journal, &fill(&admitted, 2, PRIVATE_VOLUME, CRASHED)).await?;
    let physical = fill(PHYSICAL, 2, PRIVATE_VOLUME, CRASHED).replace("{nonce}", NONCE);
    unguarded(journal, &physical).await?;
    let guard = trigger_sql(journal, "storage_initialization_insert_guard").await?;
    execute(journal, "DROP TRIGGER storage_initialization_insert_guard").await?;
    for (reservation, shape) in LIVE_RESERVATIONS.zip(LIVE) {
        let mut rows = vec![RESERVATION, INTERRUPTED];
        if shape.claimed_container {
            rows.push(CLAIMED_CONTAINER);
        }
        let volume = format!("w{reservation:032x}");
        execute_all(
            journal,
            &fill(&rows.join(";\n"), reservation, &volume, shape),
        )
        .await?;
    }
    execute(journal, &guard).await?;
    Ok((scope, root))
}

async fn unguarded(journal: &Journal, statements: &str) -> TestResult<()> {
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
        execute(journal, &format!("DROP TRIGGER {name}")).await?;
    }
    execute_all(journal, statements).await?;
    for (_, sql) in saved {
        execute(journal, &sql).await?;
    }
    Ok(())
}

async fn execute_all(journal: &Journal, rows: &str) -> TestResult<()> {
    for row in rows.split(";\n") {
        execute(journal, row).await?;
    }
    Ok(())
}

async fn execute(journal: &Journal, sql: &str) -> TestResult<()> {
    journal.connection().await?.execute(sql, ()).await?;
    Ok(())
}

async fn trigger_sql(journal: &Journal, name: &str) -> TestResult<String> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query("SELECT sql FROM sqlite_schema WHERE name = ?1", [name])
        .await?;
    let row = rows.next().await?.ok_or("trigger missing")?;
    Ok(row.get::<String>(0)?)
}

pub(super) async fn recorded(
    journal: &Journal,
    reservation: i64,
) -> TestResult<Vec<Option<String>>> {
    let connection = journal.connection().await?;
    let mut rows = connection.query(RECORDED, [reservation]).await?;
    let row = rows.next().await?.ok_or("reservation missing")?;
    Ok((0..7)
        .map(|index| row.get::<Option<String>>(index))
        .collect::<Result<_, _>>()?)
}

pub(super) async fn cleanup_proven(
    journal: &Journal,
    reservation: i64,
) -> TestResult<(i64, i64, i64)> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT reservation.remote_cleanup_proven, reservation.local_cleanup_proven, intent.cleanup_proven FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id WHERE reservation.id = ?1",
            [reservation],
        )
        .await?;
    let row = rows.next().await?.ok_or("reservation missing")?;
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}
