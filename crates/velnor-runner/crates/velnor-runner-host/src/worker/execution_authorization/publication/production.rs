//! Validate one measured publisher result before it becomes a journal receipt.

use std::{collections::HashMap, time::Instant};

use bollard::{Docker, models::VolumeScopeEnum};
use sha2::{Digest, Sha256};
use velnor_resource_probe::execution_publication::{
    ExecutionPublicationFile, ExecutionPublicationKind, ExecutionPublicationRecord,
};

use crate::{
    error::HostError,
    journal::{
        VerifiedMetadataInitialization,
        execution_authorization::{
            ExecutionChallengePublicationClaim, ExecutionDecisionDeliveryClaim,
        },
    },
    launch::{SelectedEngineContext, VerifiedWorkerImages},
    worker::execution_authorization::channel::VerifiedProofChannelBinding,
};

use super::{
    CHALLENGE_PATH, CONTROL_FILE_MODE, ControlFileReceipt, DECISION_PATH,
    ExecutionChallengePublicationReceipt, ExecutionDecisionDeliveryReceipt, MAX_CONTROL_FILE_BYTES,
    ROOT_UID,
};

const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;
const CONTROL_SUBPATH: &str = "/source-proof/control";

struct PublicationContext<'a> {
    channel: &'a VerifiedProofChannelBinding,
    metadata: &'a VerifiedMetadataInitialization,
    volume: &'a VerifiedMetadataVolume,
    helper_operation_id: &'a str,
    guest_boot_id: &'a str,
}

struct ExpectedPublicationFile<'a> {
    kind: ExecutionPublicationKind,
    expected_path: &'a str,
    helper_operation_id: &'a str,
    guest_boot_id: &'a str,
    payload: &'a [u8],
    expected_digest: &'a str,
    expected_directory:
        &'a velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory,
    volume: &'a VerifiedMetadataVolume,
}

pub(crate) struct VerifiedMetadataVolume {
    name: String,
    id: String,
    mountpoint: String,
}

impl VerifiedMetadataVolume {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn mountpoint(&self) -> &str {
        &self.mountpoint
    }
}

pub(crate) async fn inspect_metadata_volume(
    docker: &Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    metadata: &VerifiedMetadataInitialization,
    deadline: Instant,
) -> Result<VerifiedMetadataVolume, HostError> {
    if metadata.layout_version() != 2
        || metadata.reservation_id() <= 0
        || metadata.operation_id() <= 0
        || metadata.connection_id().is_empty()
        || metadata.engine_id() != engine.engine_id()
        || metadata.docker_root_digest() != engine.docker_root_digest()
        || metadata.metadata_volume_name().is_empty()
        || metadata.metadata_volume_id() != metadata.metadata_volume_name()
        || metadata.layout_digest().len() != 64
        || !metadata
            .layout_digest()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || metadata.operation_key().is_empty()
    {
        return Err(HostError::Identity);
    }
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    let volume = within(
        deadline,
        docker.inspect_volume(metadata.metadata_volume_name()),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    let expected_labels = HashMap::from([
        (
            "velnor.connection".to_owned(),
            metadata.connection_id().to_owned(),
        ),
        (
            "velnor.reservation".to_owned(),
            metadata.reservation_id().to_string(),
        ),
        (
            "velnor.operation".to_owned(),
            metadata.operation_id().to_string(),
        ),
        ("velnor.role".to_owned(), "engine-metadata".to_owned()),
    ]);
    if volume.name != metadata.metadata_volume_name()
        || volume.driver != "local"
        || volume.options != HashMap::new()
        || volume.labels != expected_labels
        || volume.scope != Some(VolumeScopeEnum::LOCAL)
        || volume.mountpoint.is_empty()
        || !volume.mountpoint.starts_with('/')
        || volume.mountpoint.chars().any(char::is_control)
        || volume.mountpoint.split('/').any(|part| part == "..")
    {
        return Err(HostError::Ownership);
    }
    Ok(VerifiedMetadataVolume {
        name: volume.name,
        id: metadata.metadata_volume_id().to_owned(),
        mountpoint: volume.mountpoint,
    })
}

pub(crate) fn challenge_publication_receipt(
    claim: &ExecutionChallengePublicationClaim,
    channel: &VerifiedProofChannelBinding,
    metadata: &VerifiedMetadataInitialization,
    record: ExecutionPublicationRecord,
    helper_operation_id: &str,
    guest_boot_id: &str,
    volume: &VerifiedMetadataVolume,
) -> Result<ExecutionChallengePublicationReceipt, HostError> {
    let bytes = claim.canonical_bytes();
    let publication = PublicationContext {
        channel,
        metadata,
        volume,
        helper_operation_id,
        guest_boot_id,
    };
    let file = publication.verified_file(
        record,
        ExecutionPublicationKind::Challenge,
        CHALLENGE_PATH,
        bytes,
        claim.challenge_digest(),
    )?;
    if claim.reservation_id() != channel.reservation_id()
        || claim.operation_id() != channel.operation_id()
        || claim.connection_id() != channel.connection_id()
        || claim.scale_set_id() != channel.scale_set_id()
        || claim.witness_request_id() != channel.witness_request_id()
        || claim.engine_id() != channel.engine_id()
        || claim.docker_root_digest() != channel.docker_root_digest()
        || claim.runner_id() != channel.runner_id()
        || claim.runner_name() != channel.runner_name()
        || claim.runner_container_id() != channel.runner_container_id()
        || claim.dind_container_id() != channel.dind_container_id()
        || claim.instance_binding_digest() != channel.instance_binding_digest()
        || claim.channel_digest() != channel.channel_digest()
    {
        return Err(HostError::Identity);
    }
    Ok(ExecutionChallengePublicationReceipt {
        reservation_id: claim.reservation_id(),
        operation_id: claim.operation_id(),
        connection_id: claim.connection_id().to_owned(),
        scale_set_id: claim.scale_set_id(),
        witness_request_id: claim.witness_request_id(),
        engine_id: claim.engine_id().to_owned(),
        docker_root_digest: claim.docker_root_digest().to_owned(),
        runner_id: claim.runner_id(),
        runner_name: claim.runner_name().to_owned(),
        runner_container_id: claim.runner_container_id().to_owned(),
        dind_container_id: claim.dind_container_id().to_owned(),
        instance_binding_digest: claim.instance_binding_digest().to_owned(),
        challenge: claim.challenge().to_owned(),
        policy_generation: u64::try_from(claim.generation()).map_err(|_| HostError::Identity)?,
        policy_digest: claim.policy_digest().to_owned(),
        channel_digest: claim.channel_digest().to_owned(),
        metadata_volume_name: metadata.metadata_volume_name().to_owned(),
        metadata_volume_id: metadata.metadata_volume_id().to_owned(),
        challenge_digest: claim.challenge_digest().to_owned(),
        file,
    })
}

pub(crate) fn decision_delivery_receipt(
    claim: &ExecutionDecisionDeliveryClaim,
    channel: &VerifiedProofChannelBinding,
    metadata: &VerifiedMetadataInitialization,
    record: ExecutionPublicationRecord,
    helper_operation_id: &str,
    guest_boot_id: &str,
    volume: &VerifiedMetadataVolume,
) -> Result<ExecutionDecisionDeliveryReceipt, HostError> {
    let bytes = claim.canonical_bytes();
    let publication = PublicationContext {
        channel,
        metadata,
        volume,
        helper_operation_id,
        guest_boot_id,
    };
    let file = publication.verified_file(
        record,
        ExecutionPublicationKind::Decision,
        DECISION_PATH,
        bytes,
        claim.decision_digest(),
    )?;
    if claim.reservation_id() != channel.reservation_id()
        || claim.operation_id() != channel.operation_id()
        || claim.connection_id() != channel.connection_id()
        || claim.scale_set_id() != channel.scale_set_id()
        || claim.witness_request_id() != channel.witness_request_id()
        || claim.engine_id() != channel.engine_id()
        || claim.docker_root_digest() != channel.docker_root_digest()
        || claim.runner_id() != channel.runner_id()
        || claim.runner_name() != channel.runner_name()
        || claim.runner_container_id() != channel.runner_container_id()
        || claim.dind_container_id() != channel.dind_container_id()
        || claim.instance_binding_digest() != channel.instance_binding_digest()
        || claim.channel_digest() != channel.channel_digest()
    {
        return Err(HostError::Identity);
    }
    Ok(ExecutionDecisionDeliveryReceipt {
        reservation_id: claim.reservation_id(),
        operation_id: claim.operation_id(),
        connection_id: claim.connection_id().to_owned(),
        scale_set_id: claim.scale_set_id(),
        witness_request_id: claim.witness_request_id(),
        actual_request_id: claim.actual_request_id(),
        engine_id: claim.engine_id().to_owned(),
        docker_root_digest: claim.docker_root_digest().to_owned(),
        runner_id: claim.runner_id(),
        runner_name: claim.runner_name().to_owned(),
        runner_container_id: claim.runner_container_id().to_owned(),
        dind_container_id: claim.dind_container_id().to_owned(),
        instance_binding_digest: claim.instance_binding_digest().to_owned(),
        challenge: claim.challenge().to_owned(),
        policy_generation: u64::try_from(claim.generation()).map_err(|_| HostError::Identity)?,
        policy_digest: claim.policy_digest().to_owned(),
        request_digest: claim.request_digest().to_owned(),
        decision_digest: claim.decision_digest().to_owned(),
        channel_digest: claim.channel_digest().to_owned(),
        metadata_volume_name: metadata.metadata_volume_name().to_owned(),
        metadata_volume_id: metadata.metadata_volume_id().to_owned(),
        file,
    })
}

impl PublicationContext<'_> {
    fn verified_file(
        &self,
        record: ExecutionPublicationRecord,
        kind: ExecutionPublicationKind,
        expected_path: &str,
        payload: &[u8],
        expected_digest: &str,
    ) -> Result<ControlFileReceipt, HostError> {
        let expected_directory = self.channel.control_directory();
        let expected = ExpectedPublicationFile {
            kind,
            expected_path,
            helper_operation_id: self.helper_operation_id,
            guest_boot_id: self.guest_boot_id,
            payload,
            expected_digest,
            expected_directory,
            volume: self.volume,
        };
        let file = verify_record(record, &expected)?;
        if self.metadata.reservation_id() != self.channel.reservation_id()
            || self.metadata.operation_id() != self.channel.operation_id()
            || self.metadata.connection_id() != self.channel.connection_id()
            || self.metadata.engine_id() != self.channel.engine_id()
            || self.metadata.docker_root_digest() != self.channel.docker_root_digest()
            || self.metadata.metadata_volume_name() != self.channel.metadata_volume_name()
            || self.metadata.metadata_volume_id() != self.channel.metadata_volume_id()
            || self.volume.name != self.metadata.metadata_volume_name()
            || self.volume.id != self.metadata.metadata_volume_id()
        {
            return Err(HostError::Identity);
        }
        Ok(ControlFileReceipt {
            path: file.path,
            inode: file.inode,
            bytes: file.bytes,
            control_directory_inode: file.control_directory_inode,
            uid: file.owner_uid,
            gid: file.owner_gid,
            mode: file.mode,
            mount_id: file.mount_id,
            device_major: file.mount_device_major,
            device_minor: file.mount_device_minor,
            statx_device_major: file.statx_device_major,
            statx_device_minor: file.statx_device_minor,
            fs_type_magic: file.filesystem_type_magic,
            filesystem_id: file.filesystem_id,
            filesystem_instance_digest: self
                .channel
                .control()
                .filesystem_instance_digest()
                .to_owned(),
            mount_source: file.mount_source,
            read_only: self.channel.control().read_only(),
            no_copy: self.channel.control().no_copy(),
        })
    }
}

fn verify_record(
    record: ExecutionPublicationRecord,
    expected: &ExpectedPublicationFile<'_>,
) -> Result<ExecutionPublicationFile, HostError> {
    let payload_digest = lower_hex(&Sha256::digest(expected.payload));
    let file = record.file;
    let device_matches = (file.mount_device_major, file.mount_device_minor)
        == (file.statx_device_major, file.statx_device_minor);
    if record.schema_version != 1
        || record.operation_id != expected.helper_operation_id
        || !valid_operation_id(expected.helper_operation_id)
        || record.guest_boot_id != expected.guest_boot_id
        || record.mount_namespace_id == 0
        || record.kind != expected.kind
        || record.payload_sha256 != expected.expected_digest
        || payload_digest != expected.expected_digest
        || expected.expected_directory.inode == 0
        || expected.expected_directory.target != "/opt/velnor/source-proof/control"
        || expected.expected_directory.owner_uid != 0
        || expected.expected_directory.owner_gid != 0
        || expected.expected_directory.mode != 0o555
        || !expected.expected_directory.read_only
        || !valid_mount_root(&expected.volume.mountpoint)
        || file.path != expected.expected_path
        || file.control_directory_inode != expected.expected_directory.inode
        || file.inode == 0
        || file.bytes != u64::try_from(expected.payload.len()).map_err(|_| HostError::Identity)?
        || file.bytes == 0
        || file.bytes > MAX_CONTROL_FILE_BYTES
        || file.owner_uid != ROOT_UID
        || file.owner_gid != ROOT_UID
        || file.mode != CONTROL_FILE_MODE
        || file.mount_id == 0
        || file.statx_mount_id != file.mount_id
        || file.filesystem_type_magic != expected.expected_directory.filesystem_type_magic
        || file.filesystem_id != expected.expected_directory.filesystem_id
        || file.mount_device_major != expected.expected_directory.mount_device_major
        || file.mount_device_minor != expected.expected_directory.mount_device_minor
        || file.statx_device_major != expected.expected_directory.statx_device_major
        || file.statx_device_minor != expected.expected_directory.statx_device_minor
        || file.mount_source != expected.expected_directory.mount_source
        || !valid_mount_root(&file.mount_root)
        || expected.expected_directory.mount_root
            != append_mount_root(&file.mount_root, CONTROL_SUBPATH)
        || file.readback_sha256 != expected.expected_digest
        || (!device_matches && file.filesystem_type_magic != BTRFS_SUPER_MAGIC)
    {
        return Err(HostError::Identity);
    }
    Ok(file)
}

#[path = "production/helpers.rs"]
mod helpers;
use helpers::{append_mount_root, lower_hex, valid_mount_root, valid_operation_id, within};

#[cfg(test)]
#[path = "production/tests.rs"]
mod tests;
