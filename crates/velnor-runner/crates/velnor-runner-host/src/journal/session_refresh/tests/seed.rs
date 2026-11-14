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
pub(super) const RESERVATION_ID: i64 = 2;
pub(super) const OPERATION_ID: i64 = 1;
pub(super) const PRIVATE_VOLUME: &str = "w0123456789abcdef0123456789abcdef";
pub(super) const IMAGE_ID: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";
pub(super) const SOURCE_SHA: &str = "1111111111111111111111111111111111111111";

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Recorded {
    pub(super) reservation: String,
    pub(super) intent: String,
    pub(super) create_dispatch: String,
    pub(super) create: String,
    pub(super) abort: String,
    pub(super) abort_helper_id: Option<String>,
}

pub(super) fn binding() -> TestResult<RepositoryConnection> {
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

pub(super) fn volume(suffix: &str) -> String {
    format!("{PRIVATE_VOLUME}{suffix}")
}

pub(super) async fn reservation_at_storage_initialization(
    journal: &Journal,
    binding: &RepositoryConnection,
    docker: &Docker,
) -> TestResult<(ConnectionJournal, String)> {
    journal.bind_engine(ENGINE_ID).await?;
    let scope = journal.connection_scope(binding).await?;
    insert(
        journal,
        "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES (?1, 7, 'ubuntu-26.04-scale-set', 'created', ?2, 'unix:///docker.sock', 'linux/amd64')",
        vec![CONNECTION_ID.into(), ENGINE_ID.into()],
    )
    .await?;
    let selected = scope.engine_context().await?.ok_or("engine context")?;
    let root = SelectedEngineContext::capture(&selected, &docker.info().await?)?
        .docker_root_digest()
        .to_owned();
    let (hex32, hex64) = ("b".repeat(32), "a".repeat(64));
    let rows: [(&str, Vec<turso::Value>); 6] = [
        (
            "INSERT INTO connection_intents (id, connection_id, kind, subject, state, scale_set_id, docker_engine_id, launch_phase, assignment_key) VALUES (?1, ?2, 'launch', 'req-11', 'pending', 7, ?3, 'provisioning_held', 'assign-1')",
            vec![OPERATION_ID.into(), CONNECTION_ID.into(), ENGINE_ID.into()],
        ),
        (
            "INSERT INTO host_storage_profiles (generation_id, profile_digest) VALUES (?1, ?2)",
            vec![hex32.as_str().into(), hex64.as_str().into()],
        ),
        (
            "INSERT INTO capacity_reservations (id, intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state, remote_cleanup_proven, local_cleanup_proven) VALUES (?1, ?2, ?3, 'assign-1', ?4, ?5, ?6, ?7, 3000, 6442450944, 'reserved', 0, 0)",
            vec![
                RESERVATION_ID.into(),
                OPERATION_ID.into(),
                CONNECTION_ID.into(),
                ENGINE_ID.into(),
                root.as_str().into(),
                scope.binding_digest().into(),
                hex32.as_str().into(),
            ],
        ),
        (
            "INSERT INTO capacity_reservation_provisioning (reservation_id, operation_id, profile_generation_id, profile_digest, sample_generation_id, observed_at_unix_ms, engine_id, docker_root_digest, retained_memory_bytes) VALUES (?1, ?2, ?3, ?4, ?3, 1700000000000, ?5, ?6, 0)",
            vec![
                RESERVATION_ID.into(),
                OPERATION_ID.into(),
                hex32.as_str().into(),
                hex64.as_str().into(),
                ENGINE_ID.into(),
                root.as_str().into(),
            ],
        ),
        (
            "INSERT INTO capacity_reservation_provisioning_topology (reservation_id, topology_version) VALUES (?1, 2)",
            vec![RESERVATION_ID.into()],
        ),
        (
            "INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state) VALUES (?1, 'not_requested')",
            vec![RESERVATION_ID.into()],
        ),
    ];
    for (sql, params) in rows {
        insert(journal, sql, params).await?;
    }
    for (role, suffix, policy) in [
        ("shared-run", "", turso::Value::Null),
        ("runner-home", "-home", turso::Value::Null),
        ("runner-work", "-work", turso::Value::Null),
        ("dind-data", "-docker", turso::Value::Null),
        ("engine-metadata", "-metadata", hex64.as_str().into()),
    ] {
        insert(
            journal,
            "INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, publication_policy_digest, create_state) VALUES (?1, ?2, ?3, ?3, ?4, 'created')",
            vec![RESERVATION_ID.into(), role.into(), volume(suffix).into(), policy],
        )
        .await?;
    }
    Ok((scope, root))
}

async fn insert(journal: &Journal, sql: &str, params: Vec<turso::Value>) -> TestResult<()> {
    in_transaction(journal, async |connection| {
        connection
            .execute(sql, params.clone())
            .await
            .map(|_| ())
            .map_err(|_| HostError::Journal)
    })
    .await?;
    Ok(())
}

pub(super) async fn recorded(journal: &Journal) -> TestResult<Recorded> {
    Ok(in_transaction(journal, async |connection| {
        let mut rows = connection
            .query(
                "SELECT reservation.state, intent.state, init.create_dispatch_state, init.create_state, init.abort_state, init.abort_helper_id FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id JOIN capacity_reservation_storage_initialization AS init ON init.reservation_id = reservation.id WHERE reservation.id = ?1",
                [RESERVATION_ID],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let text = |index| row.get::<String>(index).map_err(|_| HostError::Journal);
        Ok(Recorded {
            reservation: text(0)?,
            intent: text(1)?,
            create_dispatch: text(2)?,
            create: text(3)?,
            abort: text(4)?,
            abort_helper_id: row
                .get::<Option<String>>(5)
                .map_err(|_| HostError::Journal)?,
        })
    })
    .await?)
}
