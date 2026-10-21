//! One-shot write claims for the exact prepared decision.

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::journal::execution_authorization::records::{
    ExecutionDecisionDeliveryClaim, ProofChannelRecord, digest, sql_i64,
};
use crate::launch::execution_authorization::PersistedExecutionDecision;
use crate::worker::execution_authorization::ExecutionDecisionDeliveryReceipt;

const DECISION_PATH: &str = "/opt/velnor/source-proof/control/decision.json";
const ROOT_MODE: u32 = 0o444;
const MAX_DECISION_BYTES: usize = 8 * 1024;
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;

pub(in crate::journal::execution_authorization) async fn claim_delivery(
    scope: &ConnectionJournal,
    operation_id: i64,
) -> Result<Option<ExecutionDecisionDeliveryClaim>, HostError> {
    if operation_id <= 0 {
        return Err(HostError::Identity);
    }
    let connection_id = scope.connection_id().as_str().to_owned();
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let Some(row) = load_prepared(connection, operation_id, &connection_id).await? else {
            return Ok(None);
        };
        validate_decision(&row)?;
        let changed = connection
            .execute(
                "UPDATE connection_execution_decisions SET delivery_state = 'requested' WHERE intent_id = ?1 AND connection_id = ?2 AND delivery_state = 'prepared' AND decision_digest = ?3",
                (operation_id, connection_id.as_str(), row.decision_digest.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Ok(None);
        }
        Ok(Some(ExecutionDecisionDeliveryClaim {
            connection_id,
            reservation_id: row.reservation_id,
            operation_id,
            scale_set_id: row.scale_set_id,
            witness_request_id: row.witness_request_id,
            actual_request_id: row.actual_request_id,
            engine_id: row.engine_id,
            docker_root_digest: row.docker_root_digest,
            runner_id: row.runner_id,
            runner_name: row.runner_name,
            runner_container_id: row.runner_container_id,
            dind_container_id: row.dind_container_id,
            instance_binding_digest: row.instance_binding_digest,
            challenge: row.challenge,
            generation: row.generation,
            policy_digest: row.policy_digest,
            request_digest: row.request_digest,
            decision_digest: row.decision_digest,
            channel_digest: row.channel_digest,
            channel_json: row.channel_json,
            decision_json: row.decision_json,
        }))
    })
    .await
}

pub(in crate::journal::execution_authorization) async fn record_delivered(
    scope: &ConnectionJournal,
    claim: &ExecutionDecisionDeliveryClaim,
    receipt: &ExecutionDecisionDeliveryReceipt,
) -> Result<(), HostError> {
    if claim.connection_id() != scope.connection_id().as_str() || !receipt_matches(claim, receipt)?
    {
        return Err(HostError::Identity);
    }
    let fs_magic = format!("{:016x}", receipt.fs_type_magic());
    let filesystem_id = format!("{:016x}", receipt.filesystem_id());
    let (inode, bytes) = receipt.file_identity();
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let changed = connection
            .execute(
                "UPDATE connection_execution_decisions SET delivery_state = 'delivered', decision_file_path = ?1, decision_file_inode = ?2, decision_file_bytes = ?3, decision_file_uid = ?4, decision_file_gid = ?5, decision_file_mode = ?6, decision_mount_id = ?7, decision_device_major = ?8, decision_device_minor = ?9, decision_statx_device_major = ?10, decision_statx_device_minor = ?11, decision_fs_type_magic = ?12, decision_filesystem_id = ?13, decision_filesystem_instance_digest = ?14, decision_read_only = 1, decision_no_copy = 1 WHERE intent_id = ?15 AND reservation_id = ?16 AND connection_id = ?17 AND scale_set_id = ?18 AND witness_request_id = ?19 AND actual_request_id = ?20 AND challenge = ?21 AND generation = ?22 AND policy_digest = ?23 AND request_digest = ?24 AND decision_digest = ?25 AND delivery_state = 'requested' AND decision_file_inode IS NULL",
                turso::params![
                    receipt.path(),
                    sql_i64(inode)?,
                    sql_i64(bytes)?,
                    i64::from(receipt.file_owner().0),
                    i64::from(receipt.file_owner().1),
                    i64::from(receipt.file_mode()),
                    sql_i64(receipt.mount_id())?,
                    i64::from(receipt.device().0),
                    i64::from(receipt.device().1),
                    i64::from(receipt.statx_device().0),
                    i64::from(receipt.statx_device().1),
                    fs_magic,
                    filesystem_id,
                    receipt.filesystem_instance_digest(),
                    claim.operation_id(),
                    claim.reservation_id(),
                    claim.connection_id(),
                    claim.scale_set_id(),
                    claim.witness_request_id(),
                    claim.actual_request_id(),
                    claim.challenge(),
                    claim.generation(),
                    claim.policy_digest(),
                    claim.request_digest(),
                    claim.decision_digest(),
                ],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        Ok(())
    })
    .await
}

pub(in crate::journal::execution_authorization) async fn mark_uncertain(
    scope: &ConnectionJournal,
    claim: &ExecutionDecisionDeliveryClaim,
) -> Result<(), HostError> {
    if claim.connection_id() != scope.connection_id().as_str() {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let changed = connection
            .execute(
                "UPDATE connection_execution_decisions SET delivery_state = 'uncertain' WHERE intent_id = ?1 AND reservation_id = ?2 AND connection_id = ?3 AND actual_request_id = ?4 AND challenge = ?5 AND request_digest = ?6 AND decision_digest = ?7 AND delivery_state = 'requested'",
                (claim.operation_id(), claim.reservation_id(), claim.connection_id(), claim.actual_request_id(), claim.challenge(), claim.request_digest(), claim.decision_digest()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        Ok(())
    })
    .await
}

struct PreparedDecision {
    reservation_id: i64,
    intent_id: i64,
    scale_set_id: i64,
    witness_request_id: i64,
    actual_request_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_id: i64,
    runner_name: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    challenge: String,
    generation: i64,
    policy_digest: String,
    request_digest: String,
    decision_digest: String,
    decision_json: String,
    channel_digest: String,
    channel_json: String,
}

async fn load_prepared(
    connection: &turso::Connection,
    operation_id: i64,
    connection_id: &str,
) -> Result<Option<PreparedDecision>, HostError> {
    let mut rows = connection
        .query(
            "SELECT decision.reservation_id, decision.scale_set_id, decision.witness_request_id, decision.actual_request_id, proof.engine_id, proof.docker_root_digest, proof.runner_id, proof.runner_name, proof.runner_container_id, proof.dind_container_id, proof.instance_binding_digest, decision.challenge, decision.generation, decision.policy_digest, decision.request_digest, decision.decision_digest, decision.decision_json, proof.channel_digest, proof.channel_json FROM connection_execution_decisions AS decision JOIN connection_execution_proofs AS proof ON proof.intent_id = decision.intent_id AND proof.reservation_id = decision.reservation_id AND proof.connection_id = decision.connection_id WHERE decision.intent_id = ?1 AND decision.connection_id = ?2 AND decision.delivery_state = 'prepared' AND proof.challenge_state = 'published' AND proof.proof_state IN ('approved', 'denied')",
            (operation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let prepared = PreparedDecision {
        reservation_id: row.get(0).map_err(|_| HostError::Journal)?,
        intent_id: operation_id,
        scale_set_id: row.get(1).map_err(|_| HostError::Journal)?,
        witness_request_id: row.get(2).map_err(|_| HostError::Journal)?,
        actual_request_id: row.get(3).map_err(|_| HostError::Journal)?,
        engine_id: row.get(4).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(5).map_err(|_| HostError::Journal)?,
        runner_id: row.get(6).map_err(|_| HostError::Journal)?,
        runner_name: row.get(7).map_err(|_| HostError::Journal)?,
        runner_container_id: row.get(8).map_err(|_| HostError::Journal)?,
        dind_container_id: row.get(9).map_err(|_| HostError::Journal)?,
        instance_binding_digest: row.get(10).map_err(|_| HostError::Journal)?,
        challenge: row.get(11).map_err(|_| HostError::Journal)?,
        generation: row.get(12).map_err(|_| HostError::Journal)?,
        policy_digest: row.get(13).map_err(|_| HostError::Journal)?,
        request_digest: row.get(14).map_err(|_| HostError::Journal)?,
        decision_digest: row.get(15).map_err(|_| HostError::Journal)?,
        decision_json: row.get(16).map_err(|_| HostError::Journal)?,
        channel_digest: row.get(17).map_err(|_| HostError::Journal)?,
        channel_json: row.get(18).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(prepared))
}

fn validate_decision(prepared: &PreparedDecision) -> Result<(), HostError> {
    let decision: PersistedExecutionDecision =
        serde_json::from_str(&prepared.decision_json).map_err(|_| HostError::Journal)?;
    let bytes = decision.canonical_json()?;
    if bytes != prepared.decision_json.as_bytes()
        || digest(&bytes) != prepared.decision_digest
        || decision.reservation_id() != prepared.reservation_id
        || decision.intent_id() != prepared.intent_id
        || decision.actual_request_id() != prepared.actual_request_id
        || decision.runner_id() != prepared.runner_id
        || decision.challenge() != prepared.challenge
        || i64::try_from(decision.policy_generation()).ok() != Some(prepared.generation)
        || decision.policy_digest() != prepared.policy_digest
        || decision.request_digest() != prepared.request_digest
        || !decision.valid()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn receipt_matches(
    claim: &ExecutionDecisionDeliveryClaim,
    receipt: &ExecutionDecisionDeliveryReceipt,
) -> Result<bool, HostError> {
    let channel = ProofChannelRecord::parse(claim.channel_json(), claim.channel_digest())?;
    let (inode, bytes) = receipt.file_identity();
    let (uid, gid) = receipt.file_owner();
    let published = super::super::records::PublishedChannelDirectory {
        volume_name: receipt.metadata_volume_name(),
        volume_id: receipt.metadata_volume_id(),
        publisher_mount_id: receipt.mount_id(),
        control_directory_inode: receipt.control_directory_inode(),
        filesystem_id: receipt.filesystem_id(),
        filesystem_instance_digest: receipt.filesystem_instance_digest(),
        device: receipt.device(),
        statx_device: receipt.statx_device(),
        fs_type_magic: receipt.fs_type_magic(),
    };
    let same_directory =
        super::super::records::publication_matches_channel_directory(&channel, &published);
    let device_binding_valid =
        receipt.device() == receipt.statx_device() || receipt.fs_type_magic() == BTRFS_SUPER_MAGIC;
    Ok(receipt.reservation_id() == claim.reservation_id()
        && receipt.operation_id() == claim.operation_id()
        && receipt.connection_id() == claim.connection_id()
        && receipt.scale_set_id() == claim.scale_set_id()
        && receipt.witness_request_id() == claim.witness_request_id()
        && receipt.actual_request_id() == claim.actual_request_id()
        && receipt.engine_id() == claim.engine_id()
        && receipt.docker_root_digest() == claim.docker_root_digest()
        && receipt.runner_id() == claim.runner_id()
        && receipt.runner_name() == claim.runner_name()
        && receipt.runner_container_id() == claim.runner_container_id()
        && receipt.dind_container_id() == claim.dind_container_id()
        && receipt.instance_binding_digest() == claim.instance_binding_digest()
        && receipt.challenge() == claim.challenge()
        && i64::try_from(receipt.policy_generation()).ok() == Some(claim.generation())
        && receipt.policy_digest() == claim.policy_digest()
        && receipt.request_digest() == claim.request_digest()
        && receipt.decision_digest() == claim.decision_digest()
        && receipt.channel_digest() == claim.channel_digest()
        && receipt.path() == DECISION_PATH
        && inode > 0
        && bytes == u64::try_from(claim.canonical_bytes().len()).map_err(|_| HostError::Journal)?
        && bytes <= MAX_DECISION_BYTES as u64
        && uid == 0
        && gid == 0
        && receipt.file_mode() == ROOT_MODE
        && receipt.read_only()
        && receipt.no_copy()
        && same_directory
        && device_binding_valid)
}
