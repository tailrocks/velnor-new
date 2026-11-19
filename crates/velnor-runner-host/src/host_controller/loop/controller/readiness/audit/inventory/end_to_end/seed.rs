// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use bollard::Docker;
use sha2::{Digest, Sha256};

use super::TestResult;
use crate::config::HostConfig;
use crate::controller_ipc::SessionPollReceipt;
use crate::journal::{ConnectionJournal, Journal};
use crate::launch::{CompletedHeaderPoll, SelectedEngineContext};

pub(super) const CONNECTION: &str = "conn-test-1";
pub(super) const ENGINE: &str = "engine-test";
pub(super) const RESERVATION: i64 = 2;
pub(super) const RUNNER_ID: &str =
    "a000000000000000000000000000000000000000000000000000000000000002";
pub(super) const DIND_ID: &str = "d000000000000000000000000000000000000000000000000000000000000002";
pub(super) const RUNNER_IMAGE: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
pub(super) const DIND_IMAGE: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";

pub(super) const CONFIG: &str = r#"schema = 2
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

const ROWS: &str = "INSERT INTO host_capacity_policy (singleton, max_jobs, max_cpu_millicores, max_memory_bytes, operational_cpu_millicores, operational_memory_bytes) VALUES (1, 1, 100000, 1000000000000, 0, 0);
INSERT INTO host_storage_profiles (generation_id, profile_digest) VALUES ('{hex32}', '{hex64}');
INSERT INTO connection_request_authorizations (connection_id, scale_set_id, request_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json, acquire_state, acquire_response_json, acquire_response_digest) VALUES ('conn-test-1', 7, 11, 1, 'repository', 'created_empty_exclusive_admin_session_v1', '{binding}', '{hex64}', '{}', 'acquired', '{}', '{hex64}');
INSERT INTO connection_intents (id, connection_id, kind, subject, state, cleanup_proven, docker_id, github_runner_id, dind_id, worker_volume, scale_set_id, runner_name, docker_engine_id, launch_phase, assignment_key, acquire_attempted, acquire_resolved, acquired, jit_requested) VALUES (1, 'conn-test-1', 'launch', '{key}', 'pending', 0, '{runner}', '21', '{dind}', 'w{nonce}', 7, 'v{nonce}', 'engine-test', 'worker_ready', '{key}', 0, 0, 0, 1);
INSERT INTO connection_runner_jits (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state, runner_id, runner_name) VALUES (1, 'conn-test-1', 7, 11, 'v{nonce}', 'ready', 21, 'v{nonce}');
INSERT INTO capacity_reservations (id, intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state) VALUES (2, 1, 'conn-test-1', '{key}', 'engine-test', '{root}', '{binding}', '{hex32}', 3000, 6442450944, 'reserved');
INSERT INTO capacity_reservation_provisioning (reservation_id, operation_id, profile_generation_id, profile_digest, sample_generation_id, observed_at_unix_ms, engine_id, docker_root_digest, retained_memory_bytes) VALUES (2, 1, '{hex32}', '{hex64}', '{hex32}', 1700000000000, 'engine-test', '{root}', 0);
INSERT INTO capacity_reservation_provisioning_topology (reservation_id, topology_version) VALUES (2, 2);
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES (2, 'shared-run', 'w{nonce}', 'w{nonce}', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES (2, 'runner-home', 'w{nonce}-home', 'w{nonce}-home', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES (2, 'runner-work', 'w{nonce}-work', 'w{nonce}-work', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES (2, 'dind-data', 'w{nonce}-docker', 'w{nonce}-docker', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, publication_policy_digest, create_state) VALUES (2, 'engine-metadata', 'w{nonce}-metadata', 'w{nonce}-metadata', '{hex64}', 'created');
INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, container_id, create_state, start_state) VALUES (2, 'runner', 'w{nonce}-runner', '{runner}', 'created', 'started');
INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, container_id, create_state, start_state) VALUES (2, 'dind', 'w{nonce}-dind', '{dind}', 'created', 'started');
INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state, runner_container_id, dind_container_id, resources_bound, instance_binding_digest, guest_boot_id) VALUES (2, 'started', '{runner}', '{dind}', 1, '{hex64}', '00000000-0000-0000-0000-000000000001');
INSERT INTO physical_attempt_lineages (lineage_id, connection_id, scale_set_id, binding_digest) VALUES (2, 'conn-test-1', 7, '{binding}');
INSERT INTO physical_attempts (lineage_id, generation, reservation_id, intent_id, physical_nonce, connection_id, scale_set_id, witness_request_id, binding_digest, source_digest, engine_id, docker_root_digest, created_at) VALUES (2, 1, 2, 1, '{nonce}', 'conn-test-1', 7, 11, '{binding}', '{hex64}', 'engine-test', '{root}', 0);
INSERT INTO physical_attempt_current (lineage_id, generation, reservation_id, intent_id) VALUES (2, 1, 2, 1);
INSERT INTO capacity_reservation_root_backing_mappings (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, sample_generation_id, role, container_id, image_runtime_id, image_source_sha, logical_mount_id, logical_filesystem_digest, backing_filesystem_digest, mapping_record, mapping_digest) VALUES (2, 'conn-test-1', 1, 'engine-test', '{root}', '{hex32}', 'runner-root', '{runner}', '{runner_image}', '{sha40}', 1, '{hex64}', '{hex64}', '{}', '{hex64}');
INSERT INTO capacity_reservation_root_backing_mappings (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, sample_generation_id, role, container_id, image_runtime_id, image_source_sha, logical_mount_id, logical_filesystem_digest, backing_filesystem_digest, mapping_record, mapping_digest) VALUES (2, 'conn-test-1', 1, 'engine-test', '{root}', '{hex32}', 'dind-root', '{dind}', '{dind_image}', '{sha40}', 1, '{hex64}', '{hex64}', '{}', '{hex64}');
INSERT INTO capacity_reservation_cleanup_observer_witnesses (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, runner_container_id, dind_container_id, instance_binding_digest, guest_boot_id, binding_json, binding_sha256) VALUES (2, 'conn-test-1', 1, 'engine-test', '{root}', '{runner}', '{dind}', '{hex64}', '00000000-0000-0000-0000-000000000001', '{}', '{hex64}')";

pub(super) fn nonce() -> String {
    format!("{RESERVATION:032x}")
}

pub(super) struct Seeded {
    pub(super) scope: ConnectionJournal,
    pub(super) selected: SelectedEngineContext,
    pub(super) poll: SessionPollReceipt,
}

pub(super) async fn started_pair(
    journal: &Journal,
    docker: &Docker,
    config: &HostConfig,
) -> TestResult<Seeded> {
    journal.bind_engine(ENGINE).await?;
    let binding = config.connections().first().ok_or("connection")?;
    let scope = journal.connection_scope(binding).await?;
    let binding_digest = hex(&Sha256::digest(binding.canonical_identity_bytes()?));
    execute(journal, BINDING).await?;
    let selected = SelectedEngineContext::capture(
        &scope.engine_context().await?.ok_or("engine context")?,
        &docker.info().await?,
    )?;
    let key = format!("v1|{CONNECTION}|7|remote|6:runner|32:{}", nonce());
    let rows = ROWS
        .replace("{hex32}", &"b".repeat(32))
        .replace("{hex64}", &"c".repeat(64))
        .replace("{sha40}", &"e".repeat(40))
        .replace("{binding}", &binding_digest)
        .replace("{root}", selected.docker_root_digest())
        .replace("{key}", &key)
        .replace("{nonce}", &nonce())
        .replace("{runner_image}", RUNNER_IMAGE)
        .replace("{dind_image}", DIND_IMAGE)
        .replace("{runner}", RUNNER_ID)
        .replace("{dind}", DIND_ID);
    let mut statements = rows.split(";\n").map(str::to_owned).collect::<Vec<_>>();
    statements.extend(attachment_rows());
    unguarded(journal, &statements).await?;
    let nonce = "0123456789abcdef0123456789abcdef";
    scope.prepare_session_create(nonce).await?;
    let claim = scope.claim_session_create(nonce).await?.ok_or("claim")?;
    let session = scope.record_created_session(&claim, "session-1", 1).await?;
    let attempt = scope
        .begin_session_pressure_poll(session.intent_id(), session.allocation(), None)
        .await?;
    let completed = CompletedHeaderPoll::for_test(attempt);
    scope
        .settle_session_poll(session.intent_id(), completed)
        .await?;
    let poll = SessionPollReceipt::for_session(CONNECTION, 7, session.intent_id(), "session-1");
    Ok(Seeded {
        scope,
        selected,
        poll,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        write!(text, "{byte:02x}").expect("a string accepts formatted text");
        text
    })
}

async fn execute(journal: &Journal, sql: &str) -> TestResult<()> {
    journal.connection().await?.execute(sql, ()).await?;
    Ok(())
}

pub(super) async fn unguarded(journal: &Journal, statements: &[String]) -> TestResult<()> {
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

fn attachment_rows() -> Vec<String> {
    let runner = [
        ("shared-run", "/run", 1, 101),
        ("runner-home", "/home/runner", 2, 102),
        ("runner-work", "/home/runner/_work", 3, 103),
        ("runner-tmp", "/tmp", 4, 104),
        ("runner-shm", "/dev/shm", 5, 105),
    ]
    .map(|(role, target, id, minor)| {
        (
            (role, "runner", Some(RUNNER_ID)),
            (target, None),
            (0, id, 0, minor),
        )
    });
    let dind = [
        ("dind-data", "/var/lib/docker", 6, 106),
        ("dind-tmp", "/tmp", 7, 107),
        ("dind-shm", "/dev/shm", 8, 108),
        ("shared-run", "/run", 9, 101),
        ("runner-home", "/home/runner", 10, 102),
        ("runner-work", "/home/runner/_work", 11, 103),
    ]
    .map(|(role, target, id, minor)| {
        (
            (role, "dind", Some(DIND_ID)),
            (target, None),
            (0, id, 0, minor),
        )
    });
    let roots = [
        (
            ("runner-root", "runner", Some(RUNNER_ID)),
            ("/", None),
            (1, 12, 8, 1),
        ),
        (
            ("dind-root", "dind", Some(DIND_ID)),
            ("/", None),
            (1, 13, 8, 1),
        ),
        (
            ("engine-metadata", "probe", None),
            ("/velnor/docker-root", Some("/var/lib/docker")),
            (1, 14, 8, 1),
        ),
    ];
    let mut statements = runner
        .into_iter()
        .chain(dind)
        .chain(roots)
        .map(|((role, observer, container), (target, source), (read_only, mount_id, major, minor))| {
            let quote = |value: Option<&str>| value.map_or("NULL".to_owned(), |text| format!("'{text}'"));
            format!(
                "INSERT INTO capacity_reservation_bound_storage_attachments (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, filesystem_id, filesystem_instance_digest) VALUES (2, '{role}', '{observer}', {}, '{target}', {}, {read_only}, {mount_id}, {major}, {minor}, {major}, {minor}, 2435016766, '0000000000000001', '{}')",
                quote(container),
                quote(source),
                "c".repeat(64)
            )
        })
        .collect::<Vec<_>>();
    for (role, container) in [("runner", RUNNER_ID), ("dind", DIND_ID)] {
        statements.push(format!(
            "INSERT INTO capacity_reservation_bound_cgroups (reservation_id, role, container_id, path, memory_max_bytes, memory_current_bytes, cpu_quota_micros, cpu_period_micros, pids_max) VALUES (2, '{role}', '{container}', '/sys/fs/cgroup/docker/{role}', 1073741824, 1024, 100000, 100000, 512)"
        ));
    }
    statements
}
