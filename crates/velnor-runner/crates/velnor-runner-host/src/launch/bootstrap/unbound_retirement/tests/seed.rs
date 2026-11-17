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

pub(crate) const CONNECTION_ID: &str = "conn-test-1";
pub(crate) const ENGINE_ID: &str = "engine-test";

const BINDING: &str = "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES ('conn-test-1', 7, 'ubuntu-26.04-scale-set', 'created', 'engine-test', 'unix:///docker.sock', 'linux/amd64')";

const PROFILE: &str = "INSERT INTO host_storage_profiles (generation_id, profile_digest) VALUES ('{hex32}', '{hex64}')";

const PAIR: &str = "INSERT INTO connection_intents (id, connection_id, kind, subject, state, scale_set_id, docker_engine_id, launch_phase, assignment_key, runner_name) VALUES ({i}, 'conn-test-1', 'launch', 'req-{i}', 'pending', 7, 'engine-test', 'provisioning_held', 'assign-{i}', 'v{nonce}');
INSERT INTO capacity_reservations (id, intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state) VALUES ({r}, {i}, 'conn-test-1', 'assign-{i}', 'engine-test', '{root}', '{binding}', '{hex32}', 3000, 6442450944, 'reserved');
INSERT INTO capacity_reservation_provisioning (reservation_id, operation_id, profile_generation_id, profile_digest, sample_generation_id, observed_at_unix_ms, engine_id, docker_root_digest, retained_memory_bytes) VALUES ({r}, {i}, '{hex32}', '{hex64}', '{hex32}', 1700000000000, 'engine-test', '{root}', 0);
INSERT INTO capacity_reservation_provisioning_topology (reservation_id, topology_version) VALUES ({r}, 2);
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'shared-run', 'w{nonce}', 'w{nonce}', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'runner-home', 'w{nonce}-home', 'w{nonce}-home', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'runner-work', 'w{nonce}-work', 'w{nonce}-work', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'dind-data', 'w{nonce}-docker', 'w{nonce}-docker', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, publication_policy_digest, create_state) VALUES ({r}, 'engine-metadata', 'w{nonce}-metadata', 'w{nonce}-metadata', '{hex64}', 'created');
INSERT INTO capacity_reservation_networks (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, network_name, labels_json, create_state, network_id) VALUES ({r}, 'conn-test-1', {i}, 'engine-test', '{root}', 'velnor-v{nonce}-private', '{\"io.velnor.connection-id\":\"conn-test-1\",\"io.velnor.engine-id\":\"engine-test\",\"io.velnor.managed\":\"true\",\"io.velnor.operation-id\":\"{i}\",\"io.velnor.reservation-id\":\"{r}\"}', 'created', '{network}');
INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, container_id, create_state, start_state) VALUES ({r}, 'runner', 'w{nonce}-runner', '{runner}', 'created', 'started');
INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, container_id, create_state, start_state) VALUES ({r}, 'dind', 'w{nonce}-dind', '{dind}', 'created', 'started');
INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state, runner_container_id, dind_container_id{bound_columns}) VALUES ({r}, 'started', '{runner}', '{dind}'{bound_values});
INSERT INTO physical_attempt_lineages (lineage_id, connection_id, scale_set_id, binding_digest) VALUES ({r}, 'conn-test-1', 7, '{binding}');
INSERT INTO physical_attempts (lineage_id, generation, reservation_id, intent_id, physical_nonce, connection_id, scale_set_id, witness_request_id, binding_digest, source_digest, engine_id, docker_root_digest, created_at) VALUES ({r}, 1, {r}, {i}, '{nonce}', 'conn-test-1', 7, {r}, '{binding}', '{hex64}', 'engine-test', '{root}', 0);
INSERT INTO physical_attempt_current (lineage_id, generation, reservation_id, intent_id) VALUES ({r}, 1, {r}, {i})";

const GUARDS: [&str; 2] = [
    "physical_attempt_insert_guard",
    "physical_attempt_current_insert_guard",
];

#[derive(Clone, Copy)]
pub(crate) struct Pair {
    pub(crate) reservation: i64,
    pub(crate) bound: bool,
}

impl Pair {
    pub(crate) fn nonce(self) -> String {
        format!("{:032x}", self.reservation)
    }

    pub(crate) fn container_id(self, role: &str) -> String {
        let digit = if role == "runner" { 'a' } else { 'd' };
        format!("{}{:063x}", digit, self.reservation)
    }

    pub(crate) fn helper_id(self) -> String {
        format!("c{:063x}", self.reservation)
    }

    pub(crate) fn helper_name(self) -> String {
        format!("velnor-init-{}-1", self.reservation - 1)
    }

    pub(crate) fn network_id(self) -> String {
        format!("e{:063x}", self.reservation)
    }

    pub(crate) fn volumes(self) -> [(String, &'static str); 5] {
        let base = format!("w{}", self.nonce());
        [
            (base.clone(), "shared-run"),
            (format!("{base}-home"), "runner-home"),
            (format!("{base}-work"), "runner-work"),
            (format!("{base}-docker"), "dind-data"),
            (format!("{base}-metadata"), "engine-metadata"),
        ]
    }
}

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

pub(crate) async fn scope(journal: &Journal) -> TestResult<ConnectionJournal> {
    Ok(journal.connection_scope(&binding()?).await?)
}

pub(crate) async fn started_pairs(
    journal: &Journal,
    docker: &Docker,
    pairs: &[Pair],
) -> TestResult<(ConnectionJournal, SelectedEngineContext)> {
    journal.bind_engine(ENGINE_ID).await?;
    let binding = binding()?;
    let scope = journal.connection_scope(&binding).await?;
    let binding_digest: String = Sha256::digest(binding.canonical_identity_bytes()?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .concat();
    execute(journal, BINDING).await?;
    let engine = SelectedEngineContext::capture(
        &scope.engine_context().await?.ok_or("engine context")?,
        &docker.info().await?,
    )?;
    let (hex32, hex64) = ("b".repeat(32), "a".repeat(64));
    execute(
        journal,
        &PROFILE
            .replace("{hex32}", &hex32)
            .replace("{hex64}", &hex64),
    )
    .await?;
    let saved = trigger_sql(journal, &GUARDS).await?;
    for name in GUARDS {
        execute(journal, &format!("DROP TRIGGER {name}")).await?;
    }
    for pair in pairs {
        let (bound_columns, bound_values) = if pair.bound {
            (
                ", resources_bound, instance_binding_digest, guest_boot_id",
                format!(", 1, '{hex64}', 'boot'"),
            )
        } else {
            ("", String::new())
        };
        let rows = PAIR
            .replace("{bound_columns}", bound_columns)
            .replace("{bound_values}", &bound_values)
            .replace("{hex32}", &hex32)
            .replace("{hex64}", &hex64)
            .replace("{root}", engine.docker_root_digest())
            .replace("{binding}", &binding_digest)
            .replace("{nonce}", &pair.nonce())
            .replace("{runner}", &pair.container_id("runner"))
            .replace("{dind}", &pair.container_id("dind"))
            .replace("{network}", &pair.network_id())
            .replace("{r}", &pair.reservation.to_string())
            .replace("{i}", &(pair.reservation - 1).to_string());
        for row in rows.split(";\n") {
            execute(journal, row).await?;
        }
    }
    for sql in saved {
        execute(journal, &sql).await?;
    }
    Ok((scope, engine))
}

pub(crate) async fn initializer(
    journal: &Journal,
    pair: Pair,
    remove_state: &str,
    abort_state: &str,
) -> TestResult<()> {
    let metadata = format!("w{}-metadata", pair.nonce());
    unguarded(
        journal,
        &[format!(
            "INSERT INTO capacity_reservation_storage_initialization (reservation_id, connection_id, operation_id, generation, engine_id, docker_root_digest, image_runtime_id, image_source_sha, nano_cpus, memory_bytes, pids_limit, home_volume, work_volume, layout_version, metadata_volume, metadata_volume_id, helper_name, helper_id, create_dispatch_state, create_state, start_state, result_state, remove_state, abort_state, operation_key) SELECT r.id, r.connection_id, r.intent_id, 1, r.engine_id, r.docker_root_digest, 'sha256:{}', '{}', 500000000, 67108864, 16, 'w{}-home', 'w{}-work', 2, '{metadata}', '{metadata}', '{}', '{}', 'dispatched', 'created', 'not_requested', 'not_requested', '{remove_state}', '{abort_state}', '{}-1' FROM capacity_reservations AS r WHERE r.id = {}",
            "1".repeat(64),
            "1".repeat(40),
            pair.nonce(),
            pair.nonce(),
            pair.helper_name(),
            pair.helper_id(),
            pair.reservation - 1,
            pair.reservation
        )],
    )
    .await
}

pub(crate) async fn attach_handoff(journal: &Journal, pair: Pair) -> TestResult<()> {
    let ready = "b".repeat(64);
    let evidence = "c".repeat(64);
    let reservation = pair.reservation;
    execute(
        journal,
        &format!("INSERT INTO capacity_reservation_storage_handoffs (reservation_id, connection_id, operation_id, generation, helper_name, helper_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, operation_key, layout_digest, ready_digest, state) SELECT reservation_id, connection_id, operation_id, generation, helper_name, helper_id, engine_id, docker_root_digest, metadata_volume, metadata_volume_id, operation_key, metadata_layout_digest, '{ready}', 'ready_held' FROM capacity_reservation_storage_initialization WHERE reservation_id = {reservation}"),
    )
    .await?;
    execute(
        journal,
        &format!("UPDATE capacity_reservation_storage_handoffs SET state = 'attached', mount_evidence_digest = '{evidence}', runner_container_id = (SELECT container_id FROM capacity_reservation_bootstrap_containers WHERE reservation_id = {reservation} AND role = 'runner'), dind_container_id = (SELECT container_id FROM capacity_reservation_bootstrap_containers WHERE reservation_id = {reservation} AND role = 'dind') WHERE reservation_id = {reservation} AND state = 'ready_held'"),
    )
    .await
}

pub(crate) async fn execute(journal: &Journal, sql: &str) -> TestResult<()> {
    journal.connection().await?.execute(sql, ()).await?;
    Ok(())
}

async fn trigger_sql(journal: &Journal, names: &[&str]) -> TestResult<Vec<String>> {
    let connection = journal.connection().await?;
    let mut saved = Vec::new();
    for &name in names {
        let mut rows = connection
            .query("SELECT sql FROM sqlite_schema WHERE name = ?1", [name])
            .await?;
        let row = rows.next().await?.ok_or("trigger missing")?;
        saved.push(row.get::<String>(0)?);
    }
    Ok(saved)
}

pub(crate) async fn unguarded(journal: &Journal, statements: &[String]) -> TestResult<()> {
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
    for sql in statements {
        execute(journal, sql).await?;
    }
    for (_, sql) in saved {
        execute(journal, &sql).await?;
    }
    Ok(())
}

const ROOT_BACKING: &str = "INSERT INTO capacity_reservation_root_backing_mappings (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, sample_generation_id, role, container_id, image_runtime_id, image_source_sha, logical_mount_id, logical_filesystem_digest, backing_filesystem_digest, mapping_record, mapping_digest) VALUES ({r}, 'conn-test-1', {i}, 'engine-test', '{root}', '{hex32}', '{role}', '{container}', 'image-test', '{sha40}', 1, '{hex64}', '{hex64}', '{}', '{hex64}')";

const WITNESS: &str = "INSERT INTO capacity_reservation_cleanup_observer_witnesses (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, runner_container_id, dind_container_id, instance_binding_digest, guest_boot_id, binding_json, binding_sha256) VALUES ({r}, 'conn-test-1', {i}, 'engine-test', '{root}', '{runner}', '{dind}', '{digest}', '{boot}', '{}', '{hex64}')";

const BOUND_SEED_GUARDS: [&str; 2] = [
    "root_backing_insert_guard",
    "cleanup_observer_witness_insert_guard",
];

pub(crate) async fn bind_resources(
    journal: &Journal,
    pair: Pair,
    docker_root_digest: &str,
) -> TestResult<()> {
    let saved = trigger_sql(journal, &BOUND_SEED_GUARDS).await?;
    for name in BOUND_SEED_GUARDS {
        execute(journal, &format!("DROP TRIGGER {name}")).await?;
    }
    let reservation = pair.reservation.to_string();
    let witness_digest = "d".repeat(64);
    let rows = [ROOT_BACKING, ROOT_BACKING, WITNESS]
        .iter()
        .enumerate()
        .map(|(index, sql)| {
            let role = if index == 1 {
                "dind-root"
            } else {
                "runner-root"
            };
            sql.replace("{r}", &reservation)
                .replace("{i}", &(pair.reservation - 1).to_string())
                .replace("{root}", docker_root_digest)
                .replace("{hex32}", &"b".repeat(32))
                .replace("{hex64}", &"c".repeat(64))
                .replace("{sha40}", &"e".repeat(40))
                .replace("{role}", role)
                .replace(
                    "{container}",
                    &pair.container_id(if index == 1 { "dind" } else { "runner" }),
                )
                .replace("{runner}", &pair.container_id("runner"))
                .replace("{dind}", &pair.container_id("dind"))
                .replace("{digest}", &witness_digest)
                .replace("{boot}", BOOT_ID)
        })
        .collect::<Vec<_>>();
    for row in rows {
        execute(journal, &row).await?;
    }
    for sql in saved {
        execute(journal, &sql).await?;
    }
    execute(
        journal,
        &format!(
            "UPDATE capacity_reservation_provisioning_state SET resources_bound = 1, instance_binding_digest = '{witness_digest}', guest_boot_id = '{BOOT_ID}' WHERE reservation_id = {reservation}"
        ),
    )
    .await
}

const BOOT_ID: &str = "00000000-0000-0000-0000-000000000001";

pub(crate) async fn resources_bound(journal: &Journal, reservation: i64) -> TestResult<i64> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT resources_bound FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            [reservation],
        )
        .await?;
    let row = rows.next().await?.ok_or("row missing")?;
    Ok(row.get::<i64>(0)?)
}

pub(crate) async fn text(journal: &Journal, sql: &str, reservation: i64) -> TestResult<String> {
    let connection = journal.connection().await?;
    let mut rows = connection.query(sql, [reservation]).await?;
    let row = rows.next().await?.ok_or("row missing")?;
    Ok(row.get::<String>(0)?)
}

pub(crate) const RESERVATION: &str = "SELECT r.state || '/' || r.remote_cleanup_proven || r.local_cleanup_proven || '/' || i.state || '/' || i.cleanup_proven || '/' || (SELECT COUNT(*) FROM physical_attempt_current AS c WHERE c.reservation_id = r.id) || '/' || COALESCE((SELECT outcome FROM physical_attempt_terminal_proofs AS p WHERE p.reservation_id = r.id), 'none') || '/' || COALESCE((SELECT group_concat(phase) FROM (SELECT phase FROM capacity_reservation_unbound_retirement_progress AS g WHERE g.reservation_id = r.id ORDER BY phase)), '') FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id WHERE r.id = ?1";
