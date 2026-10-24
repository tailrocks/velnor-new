//! Stored proof-channel mount records bound to the physical runner.

use crate::error::HostError;
use crate::journal::execution_authorization::ProofChannelRecord;

pub(super) struct ProofChannelIdentity<'a> {
    pub(super) reservation_id: i64,
    pub(super) intent_id: i64,
    pub(super) connection_id: &'a str,
    pub(super) scale_set_id: i64,
    pub(super) witness_request_id: i64,
    pub(super) engine_id: &'a str,
    pub(super) docker_root_digest: &'a str,
    pub(super) runner_id: i64,
    pub(super) runner_name: &'a str,
    pub(super) runner_container_id: &'a str,
    pub(super) dind_container_id: &'a str,
    pub(super) instance_binding_digest: &'a str,
}

pub(super) async fn validate(
    connection: &turso::Connection,
    json: &str,
    expected_digest: &str,
    identity: &ProofChannelIdentity<'_>,
) -> Result<(), HostError> {
    let record = ProofChannelRecord::parse(json, expected_digest)?;
    if !matches_identity(&record, identity) {
        return Err(HostError::Journal);
    }
    validate_owned_volume(connection, identity.reservation_id, &record).await
}

fn matches_identity(record: &ProofChannelRecord, expected: &ProofChannelIdentity<'_>) -> bool {
    record.valid()
        && record.reservation_id() == expected.reservation_id
        && record.operation_id() == expected.intent_id
        && record.connection_id() == expected.connection_id
        && record.scale_set_id() == expected.scale_set_id
        && record.witness_request_id() == expected.witness_request_id
        && record.engine_id() == expected.engine_id
        && record.docker_root_digest() == expected.docker_root_digest
        && record.runner_id() == expected.runner_id
        && record.runner_name() == expected.runner_name
        && record.runner_container_id() == expected.runner_container_id
        && record.dind_container_id() == expected.dind_container_id
        && record.instance_binding_digest() == expected.instance_binding_digest
}

async fn validate_owned_volume(
    connection: &turso::Connection,
    reservation_id: i64,
    record: &ProofChannelRecord,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT docker_name, docker_id, publication_policy_digest, create_state FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = 'engine-metadata'",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Journal);
    };
    let name: String = row.get(0).map_err(|_| HostError::Journal)?;
    let object_id: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let publication_policy: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    let create_state: String = row.get(3).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || name != record.metadata_volume_name()
        || object_id.as_deref() != Some(record.metadata_volume_id())
        || publication_policy
            .as_deref()
            .is_none_or(|digest| !valid_digest(digest))
        || create_state != "created"
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
