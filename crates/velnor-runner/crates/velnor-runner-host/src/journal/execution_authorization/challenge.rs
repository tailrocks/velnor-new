//! One-shot durable challenge publication for a verified Runner channel.

mod context;

use context::{
    challenge_context, channel_matches_context, insert_requested_proof, metadata_volume_matches,
    proof_exists,
};

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::journal::execution_authorization::records::{
    ExecutionChallengePublicationClaim, ProofChannelRecord, digest, sql_i64,
};
use crate::launch::execution_authorization::{
    ExecutionProofChallenge, ExecutionProofChallengeFields,
};
use crate::worker::execution_authorization::{
    ExecutionChallengePublicationReceipt, VerifiedProofChannelBinding,
};

const MAX_CHANNEL_BYTES: usize = 16 * 1024;
const MAX_CHALLENGE_BYTES: usize = 8 * 1024;
const CONTROL_PATH: &str = "/opt/velnor/source-proof/control/challenge.json";
const CONTROL_MODE: u32 = 0o444;
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;
const HEX: &[u8; 16] = b"0123456789abcdef";

struct PreparedChannel {
    json: String,
    digest: String,
}

pub(super) async fn claim(
    scope: &ConnectionJournal,
    reservation_id: i64,
    channel: &VerifiedProofChannelBinding,
) -> Result<Option<ExecutionChallengePublicationClaim>, HostError> {
    let prepared = prepare_channel(scope, reservation_id, channel)?;
    let connection_id = scope.connection_id().as_str().to_owned();
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        claim_in_transaction(
            connection,
            reservation_id,
            &connection_id,
            channel,
            &prepared,
        )
        .await
    })
    .await
}

fn prepare_channel(
    scope: &ConnectionJournal,
    reservation_id: i64,
    channel: &VerifiedProofChannelBinding,
) -> Result<PreparedChannel, HostError> {
    if reservation_id <= 0 || !channel.valid() || channel.reservation_id() != reservation_id {
        return Err(HostError::Identity);
    }
    let json = String::from_utf8(channel.canonical_json()?).map_err(|_| HostError::Journal)?;
    let digest = channel.computed_digest()?;
    if json.len() > MAX_CHANNEL_BYTES {
        return Err(HostError::Identity);
    }
    let durable = ProofChannelRecord::parse(&json, &digest)?;
    if durable.reservation_id() != reservation_id
        || durable.operation_id() <= 0
        || durable.connection_id() != scope.connection_id().as_str()
    {
        return Err(HostError::Identity);
    }
    Ok(PreparedChannel { json, digest })
}

async fn claim_in_transaction(
    connection: &turso::Connection,
    reservation_id: i64,
    connection_id: &str,
    channel: &VerifiedProofChannelBinding,
    prepared: &PreparedChannel,
) -> Result<Option<ExecutionChallengePublicationClaim>, HostError> {
    let Some(context) = challenge_context(connection, reservation_id, connection_id).await? else {
        return Ok(None);
    };
    if !channel_matches_context(channel, &context)
        || !metadata_volume_matches(connection, reservation_id, channel).await?
        || proof_exists(connection, reservation_id).await?
    {
        return Ok(None);
    }
    let challenge = random_challenge()?;
    let challenge_document =
        ExecutionProofChallenge::from_journal(ExecutionProofChallengeFields {
            challenge: challenge.clone(),
            reservation_id,
            intent_id: context.intent_id,
            witness_request_id: context.witness_request_id,
            runner_id: context.runner_id,
            runner_name: context.runner_name.clone(),
            runner_scale_set_id: context.scale_set_id,
            runner_container_id: context.runner_container_id.clone(),
            dind_container_id: context.dind_container_id.clone(),
            engine_id: context.engine_id.clone(),
            docker_root_digest: context.docker_root_digest.clone(),
            instance_binding_digest: context.instance_binding_digest.clone(),
            policy_generation: u64::try_from(context.generation).map_err(|_| HostError::Journal)?,
            policy_digest: context.policy_digest.clone(),
            channel_digest: prepared.digest.clone(),
        })?;
    let challenge_bytes = challenge_document.canonical_json()?;
    if challenge_bytes.len() > MAX_CHALLENGE_BYTES {
        return Err(HostError::Journal);
    }
    let challenge_json =
        String::from_utf8(challenge_bytes.clone()).map_err(|_| HostError::Journal)?;
    let challenge_digest = digest(&challenge_bytes);
    insert_requested_proof(
        connection,
        &context,
        &challenge,
        &prepared.digest,
        &prepared.json,
        &challenge_json,
        &challenge_digest,
    )
    .await?;
    Ok(Some(ExecutionChallengePublicationClaim {
        connection_id: context.connection_id,
        reservation_id,
        operation_id: context.intent_id,
        scale_set_id: context.scale_set_id,
        witness_request_id: context.witness_request_id,
        engine_id: context.engine_id,
        docker_root_digest: context.docker_root_digest,
        runner_id: context.runner_id,
        runner_name: context.runner_name,
        runner_container_id: context.runner_container_id,
        dind_container_id: context.dind_container_id,
        instance_binding_digest: context.instance_binding_digest,
        channel_json: prepared.json.clone(),
        challenge,
        generation: context.generation,
        policy_digest: context.policy_digest,
        channel_digest: prepared.digest.clone(),
        challenge_json,
        challenge_digest,
    }))
}

pub(super) async fn record_published(
    scope: &ConnectionJournal,
    claim: &ExecutionChallengePublicationClaim,
    receipt: &ExecutionChallengePublicationReceipt,
) -> Result<(), HostError> {
    if claim.connection_id() != scope.connection_id().as_str() || !receipt_matches(claim, receipt)?
    {
        return Err(HostError::Identity);
    }
    let fs_magic = format!("{:016x}", receipt.fs_type_magic());
    let filesystem_id = format!("{:016x}", receipt.filesystem_id());
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let changed = connection
            .execute(
                "UPDATE connection_execution_proofs SET challenge_state = 'published', challenge_file_path = ?1, challenge_file_inode = ?2, challenge_file_bytes = ?3, challenge_file_uid = ?4, challenge_file_gid = ?5, challenge_file_mode = ?6, challenge_mount_id = ?7, challenge_device_major = ?8, challenge_device_minor = ?9, challenge_statx_device_major = ?10, challenge_statx_device_minor = ?11, challenge_fs_type_magic = ?12, challenge_filesystem_id = ?13, challenge_filesystem_instance_digest = ?14, challenge_read_only = 1, challenge_no_copy = 1 WHERE intent_id = ?15 AND reservation_id = ?16 AND connection_id = ?17 AND challenge = ?18 AND challenge_state = 'requested' AND challenge_digest = ?19 AND channel_digest = ?20",
                turso::params![
                    receipt.path(),
                    sql_i64(receipt.file_identity().0)?,
                    sql_i64(receipt.file_identity().1)?,
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
                    claim.challenge(),
                    claim.challenge_digest(),
                    claim.channel_digest(),
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

pub(super) async fn mark_uncertain(
    scope: &ConnectionJournal,
    claim: &ExecutionChallengePublicationClaim,
) -> Result<(), HostError> {
    if claim.connection_id() != scope.connection_id().as_str() {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let changed = connection
            .execute(
                "UPDATE connection_execution_proofs SET challenge_state = 'uncertain' WHERE intent_id = ?1 AND reservation_id = ?2 AND connection_id = ?3 AND challenge = ?4 AND challenge_state = 'requested' AND challenge_digest = ?5 AND channel_digest = ?6",
                (claim.operation_id(), claim.reservation_id(), claim.connection_id(), claim.challenge(), claim.challenge_digest(), claim.channel_digest()),
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

fn receipt_matches(
    claim: &ExecutionChallengePublicationClaim,
    receipt: &ExecutionChallengePublicationReceipt,
) -> Result<bool, HostError> {
    let channel = ProofChannelRecord::parse(claim.channel_json(), claim.channel_digest())?;
    let (inode, bytes) = receipt.file_identity();
    let (uid, gid) = receipt.file_owner();
    let device = receipt.device();
    let statx_device = receipt.statx_device();
    let published = super::records::PublishedChannelDirectory {
        volume_name: receipt.metadata_volume_name(),
        volume_id: receipt.metadata_volume_id(),
        publisher_mount_id: receipt.mount_id(),
        control_directory_inode: receipt.control_directory_inode(),
        filesystem_id: receipt.filesystem_id(),
        filesystem_instance_digest: receipt.filesystem_instance_digest(),
        device,
        statx_device,
        fs_type_magic: receipt.fs_type_magic(),
    };
    let same_filesystem =
        super::records::publication_matches_channel_directory(&channel, &published);
    let device_binding_valid =
        device == statx_device || receipt.fs_type_magic() == BTRFS_SUPER_MAGIC;
    Ok(receipt.reservation_id() == claim.reservation_id()
        && receipt.operation_id() == claim.operation_id()
        && receipt.connection_id() == claim.connection_id()
        && receipt.scale_set_id() == claim.scale_set_id()
        && receipt.witness_request_id() == claim.witness_request_id()
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
        && receipt.channel_digest() == claim.channel_digest()
        && receipt.challenge_digest() == claim.challenge_digest()
        && receipt.path() == CONTROL_PATH
        && inode > 0
        && bytes == u64::try_from(claim.canonical_bytes().len()).map_err(|_| HostError::Journal)?
        && bytes <= MAX_CHALLENGE_BYTES as u64
        && uid == 0
        && gid == 0
        && receipt.file_mode() == CONTROL_MODE
        && receipt.read_only()
        && receipt.no_copy()
        && same_filesystem
        && device_binding_valid)
}

fn random_challenge() -> Result<String, HostError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| HostError::Identity)?;
    let mut challenge = String::with_capacity(64);
    for byte in bytes {
        challenge.push(char::from(HEX[usize::from(byte >> 4)]));
        challenge.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(challenge)
}
