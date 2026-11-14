// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::{ConnectionJournal, Journal};
use crate::launch::SelectedEngineContext;

use super::TestResult;

pub(super) const CONNECTION_ID: &str = "conn-test-1";
pub(super) const ENGINE_ID: &str = "engine-test";
pub(super) const PRIVATE_VOLUME: &str = "w0123456789abcdef0123456789abcdef";
const PAIR_STAGE_VOLUME: &str = "wfedcba9876543210fedcba9876543210";
pub(super) const IMAGE_ID: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

const BINDING: &str = "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES ('conn-test-1', 7, 'ubuntu-26.04-scale-set', 'created', 'engine-test', 'unix:///docker.sock', 'linux/amd64')";

const RESERVATION: &str = "INSERT INTO connection_intents (id, connection_id, kind, subject, state, scale_set_id, docker_engine_id, launch_phase, assignment_key) VALUES ({i}, 'conn-test-1', 'launch', 'req-{i}', 'pending', 7, 'engine-test', 'provisioning_held', 'assign-{i}');
INSERT INTO capacity_reservations (id, intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state) VALUES ({r}, {i}, 'conn-test-1', 'assign-{i}', 'engine-test', '{root}', '{binding}', '{hex32}', 3000, 6442450944, 'reserved');
INSERT INTO capacity_reservation_provisioning (reservation_id, operation_id, profile_generation_id, profile_digest, sample_generation_id, observed_at_unix_ms, engine_id, docker_root_digest, retained_memory_bytes) VALUES ({r}, {i}, '{hex32}', '{hex64}', '{hex32}', 1700000000000, 'engine-test', '{root}', 0);
INSERT INTO capacity_reservation_provisioning_topology (reservation_id, topology_version) VALUES ({r}, 2);
INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state) VALUES ({r}, 'not_requested');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'shared-run', '{volume}', '{volume}', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'runner-home', '{volume}-home', '{volume}-home', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'runner-work', '{volume}-work', '{volume}-work', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, create_state) VALUES ({r}, 'dind-data', '{volume}-docker', '{volume}-docker', 'created');
INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, publication_policy_digest, create_state) VALUES ({r}, 'engine-metadata', '{volume}-metadata', '{volume}-metadata', '{hex64}', 'created')";

const PAIR_STAGE: &str = "INSERT INTO capacity_reservation_storage_initialization (reservation_id, connection_id, operation_id, generation, engine_id, docker_root_digest, image_runtime_id, image_source_sha, nano_cpus, memory_bytes, pids_limit, home_volume, work_volume, layout_version, metadata_volume, metadata_volume_id, helper_name, create_dispatch_state, create_state, helper_id, start_state, result_state, remove_state, operation_key, home_device_major, home_device_minor, home_inode, home_mount_id, work_device_major, work_device_minor, work_inode, work_mount_id, metadata_layout_digest) VALUES ({r}, 'conn-test-1', {i}, 1, 'engine-test', '{root}', '{image}', '{hex40}', 500000000, 67108864, 16, '{volume}-home', '{volume}-work', 2, '{volume}-metadata', '{volume}-metadata', 'velnor-init-{i}-1', 'response_received', 'created', '{hex64}', 'started', 'confirmed', 'not_requested', '{i}-1', 0, 101, 1, 1, 0, 102, 2, 2, '{hex64}');
INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, create_state, start_state) VALUES ({r}, 'runner', '{volume}-runner', 'requested', 'not_requested')";

const RECORDED: &str = "SELECT reservation.state, intent.state, init.create_dispatch_state, init.create_state, init.abort_state, init.abort_helper_id FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id JOIN capacity_reservation_storage_initialization AS init ON init.reservation_id = reservation.id WHERE reservation.id = ?1";

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
    let scope = journal.connection_scope(&binding()?).await?;
    execute(journal, BINDING).await?;
    let selected = scope.engine_context().await?.ok_or("engine context")?;
    let root = SelectedEngineContext::capture(&selected, &docker.info().await?)?
        .docker_root_digest()
        .to_owned();
    execute(
        journal,
        &format!(
            "INSERT INTO host_storage_profiles (generation_id, profile_digest) VALUES ('{}', '{}')",
            "b".repeat(32),
            "a".repeat(64)
        ),
    )
    .await?;
    for (reservation, intent, volume, rows) in [
        (2, 1, PRIVATE_VOLUME, RESERVATION.to_owned()),
        (
            3,
            2,
            PAIR_STAGE_VOLUME,
            format!("{RESERVATION};\n{PAIR_STAGE}"),
        ),
    ] {
        let rows = rows
            .replace("{hex32}", &"b".repeat(32))
            .replace("{hex40}", &"1".repeat(40))
            .replace("{hex64}", &"a".repeat(64))
            .replace("{root}", &root)
            .replace("{image}", IMAGE_ID)
            .replace("{binding}", scope.binding_digest())
            .replace("{volume}", volume)
            .replace("{r}", &reservation.to_string())
            .replace("{i}", &intent.to_string());
        for row in rows.split(";\n") {
            execute(journal, row).await?;
        }
    }
    Ok((scope, root))
}

async fn execute(journal: &Journal, sql: &str) -> TestResult<()> {
    in_transaction(journal, async |connection| {
        connection
            .execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)
    })
    .await?;
    Ok(())
}

pub(super) async fn recorded(
    journal: &Journal,
    reservation: i64,
) -> TestResult<Vec<Option<String>>> {
    Ok(in_transaction(journal, async |connection| {
        let mut rows = connection
            .query(RECORDED, [reservation])
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        (0..6)
            .map(|index| row.get::<Option<String>>(index))
            .collect::<Result<_, _>>()
            .map_err(|_| HostError::Journal)
    })
    .await?)
}
