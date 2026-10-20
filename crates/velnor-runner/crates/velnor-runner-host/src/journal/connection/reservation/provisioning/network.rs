use std::collections::BTreeMap;

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::launch::bootstrap::{VerifiedStorageNetworkCreated, VerifiedStorageNetworkRemoved};

/// One durable request to create a private network for a reserved worker.
pub(crate) struct StorageNetworkCreateClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    network_name: String,
    labels: BTreeMap<String, String>,
}

impl StorageNetworkCreateClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn network_name(&self) -> &str {
        &self.network_name
    }
    pub(crate) fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }
}

/// Durable identity for a network that passed the host inspection checks.
pub(crate) struct RecordedStorageNetwork {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    network_name: String,
    network_id: String,
    labels: BTreeMap<String, String>,
}

impl RecordedStorageNetwork {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn network_name(&self) -> &str {
        &self.network_name
    }
    pub(crate) fn network_id(&self) -> &str {
        &self.network_id
    }
    pub(crate) fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }
}

/// A read-only identity for explicit recovery after an interrupted network call.
pub(crate) struct StorageNetworkRecoveryIdentity {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    network_name: String,
    network_id: Option<String>,
    labels: BTreeMap<String, String>,
    create_state: String,
    remove_state: String,
}

impl StorageNetworkRecoveryIdentity {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn network_name(&self) -> &str {
        &self.network_name
    }
    pub(crate) fn network_id(&self) -> Option<&str> {
        self.network_id.as_deref()
    }
    pub(crate) fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }
    pub(crate) fn create_state(&self) -> &str {
        &self.create_state
    }
    pub(crate) fn remove_state(&self) -> &str {
        &self.remove_state
    }
}

/// Permission to remove one recorded private network after worker cleanup.
pub(crate) struct StorageNetworkRemovalClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    network_name: String,
    network_id: Option<String>,
    labels: BTreeMap<String, String>,
}

impl StorageNetworkRemovalClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn network_name(&self) -> &str {
        &self.network_name
    }
    pub(crate) fn network_id(&self) -> Option<&str> {
        self.network_id.as_deref()
    }
    pub(crate) fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }
}

impl ConnectionJournal {
    /// Record the private network intent before the host calls Docker.
    pub(crate) async fn claim_storage_network_create(
        &self,
        reservation_id: i64,
    ) -> Result<Option<StorageNetworkCreateClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = super::hold_context(connection, &connection_id, reservation_id).await? else {
                return Ok(None);
            };
            if !super::hold_is_claimable(&context) { return Ok(None); }
            let mut rows = connection.query(
                "SELECT i.runner_name FROM connection_intents AS i WHERE i.id = ?1 AND i.connection_id = ?2 AND i.kind = 'launch' AND i.state = 'pending' AND i.cleanup_proven = 0 AND (NOT EXISTS (SELECT 1 FROM physical_attempt_current AS current WHERE current.reservation_id = ?3) OR EXISTS (SELECT 1 FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id WHERE current.reservation_id = ?3 AND auth.acquire_state = 'acquired'))",
                (context.operation_id, connection_id.as_str(), reservation_id),
            ).await.map_err(|_| HostError::Journal)?;
            let runner_name: String = rows.next().await.map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?.get(0).map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || !valid_runner_name(&runner_name) {
                return Err(HostError::Identity);
            }
            let network_name = format!("velnor-{runner_name}-private");
            let labels = network_labels(reservation_id, context.operation_id, &connection_id, &context.engine_id);
            let labels_json = serde_json::to_string(&labels).map_err(|_| HostError::Journal)?;
            let changed = connection.execute(
                "INSERT INTO capacity_reservation_networks (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, network_name, labels_json, create_state, remove_state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'requested', 'not_requested') ON CONFLICT(reservation_id) DO NOTHING",
                (reservation_id, connection_id.as_str(), context.operation_id, context.engine_id.as_str(), context.docker_root_digest.as_str(), network_name.as_str(), labels_json.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Ok(None); }
            Ok(Some(StorageNetworkCreateClaim {
                connection_id: connection_id.clone(), reservation_id,
                operation_id: context.operation_id, engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest, network_name, labels,
            }))
        }).await
    }

    /// Record only a network that passed exact daemon and object inspection.
    pub(crate) async fn record_storage_network_created(
        &self,
        claim: &StorageNetworkCreateClaim,
        proof: &VerifiedStorageNetworkCreated,
    ) -> Result<RecordedStorageNetwork, HostError> {
        validate_create_claim(self, claim)?;
        if proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || proof.network_name() != claim.network_name
            || proof.labels() != &claim.labels
            || !valid_network_id(proof.network_id())
        {
            return Err(HostError::Identity);
        }
        let network_id = proof.network_id().to_owned();
        let labels_json = serde_json::to_string(&claim.labels).map_err(|_| HostError::Journal)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection.execute(
                "UPDATE capacity_reservation_networks SET create_state = 'created', network_id = ?1 WHERE reservation_id = ?2 AND connection_id = ?3 AND operation_id = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND network_name = ?7 AND labels_json = ?8 AND create_state IN ('requested', 'uncertain') AND network_id IS NULL AND remove_state = 'not_requested'",
                (network_id.as_str(), claim.reservation_id, claim.connection_id.as_str(), claim.operation_id, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.network_name.as_str(), labels_json.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            Ok(RecordedStorageNetwork {
                connection_id: claim.connection_id.clone(), reservation_id: claim.reservation_id,
                operation_id: claim.operation_id, engine_id: claim.engine_id.clone(),
                docker_root_digest: claim.docker_root_digest.clone(), network_name: claim.network_name.clone(),
                network_id: network_id.clone(), labels: claim.labels.clone(),
            })
        }).await
    }
}

mod recovery;
pub(super) use recovery::release_ready;

fn network_labels(
    reservation_id: i64,
    operation_id: i64,
    connection_id: &str,
    engine_id: &str,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("io.velnor.managed".to_owned(), "true".to_owned()),
        (
            "io.velnor.connection-id".to_owned(),
            connection_id.to_owned(),
        ),
        ("io.velnor.engine-id".to_owned(), engine_id.to_owned()),
        (
            "io.velnor.operation-id".to_owned(),
            operation_id.to_string(),
        ),
        (
            "io.velnor.reservation-id".to_owned(),
            reservation_id.to_string(),
        ),
    ])
}

fn parse_labels(value: &str) -> Result<BTreeMap<String, String>, HostError> {
    let labels: BTreeMap<String, String> =
        serde_json::from_str(value).map_err(|_| HostError::Journal)?;
    if serde_json::to_string(&labels).map_err(|_| HostError::Journal)? != value {
        return Err(HostError::Journal);
    }
    Ok(labels)
}

fn valid_runner_name(value: &str) -> bool {
    value.len() == 33
        && value.starts_with('v')
        && value[1..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_network_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_create_claim(
    journal: &ConnectionJournal,
    claim: &StorageNetworkCreateClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}

fn validate_remove_claim(
    journal: &ConnectionJournal,
    claim: &StorageNetworkRemovalClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}
