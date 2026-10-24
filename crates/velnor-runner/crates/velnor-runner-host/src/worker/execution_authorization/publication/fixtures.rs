use super::{
    BTRFS_SUPER_MAGIC, CHALLENGE_PATH, CONTROL_FILE_MODE, ControlFileReceipt,
    ExecutionChallengePublicationReceipt, MAX_CONTROL_FILE_BYTES, ROOT_UID,
};
use crate::error::HostError;
use crate::{
    journal::execution_authorization::ExecutionChallengePublicationClaim,
    worker::execution_authorization::channel::{
        VerifiedProofChannelBinding, valid_digest, valid_engine, valid_object_id,
    },
};

#[cfg(test)]
pub(crate) struct TestControlFile {
    pub(crate) path: String,
    pub(crate) inode: u64,
    pub(crate) bytes: u64,
    pub(crate) control_directory_inode: u64,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) mode: u32,
    pub(crate) mount_id: u64,
    pub(crate) device: (u32, u32),
    pub(crate) statx_device: (u32, u32),
    pub(crate) fs_type_magic: u64,
    pub(crate) filesystem_id: u64,
    pub(crate) filesystem_instance_digest: String,
    pub(crate) mount_source: String,
    pub(crate) read_only: bool,
    pub(crate) no_copy: bool,
}

fn validate_control_file(file: &ControlFileReceipt, path: &str) -> bool {
    file.path == path
        && file.inode > 0
        && file.bytes > 0
        && file.control_directory_inode > 0
        && file.bytes <= MAX_CONTROL_FILE_BYTES
        && file.uid == ROOT_UID
        && file.gid == ROOT_UID
        && file.mode == CONTROL_FILE_MODE
        && file.mount_id > 0
        && file.fs_type_magic > 0
        && file.filesystem_id > 0
        && valid_digest(&file.filesystem_instance_digest)
        && !file.mount_source.is_empty()
        && !file.mount_source.chars().any(char::is_control)
        && file.read_only
        && file.no_copy
        && ((file.device_major, file.device_minor)
            == (file.statx_device_major, file.statx_device_minor)
            || file.fs_type_magic == BTRFS_SUPER_MAGIC)
}

struct CommonReceiptIdentity<'a> {
    connection_id: &'a str,
    reservation_id: i64,
    operation_id: i64,
    scale_set_id: i64,
    witness_request_id: i64,
    engine_id: &'a str,
    docker_root_digest: &'a str,
    runner_id: i64,
    runner_name: &'a str,
    runner_container_id: &'a str,
    dind_container_id: &'a str,
    instance_binding_digest: &'a str,
    challenge: &'a str,
    policy_generation: u64,
    policy_digest: &'a str,
    channel_digest: &'a str,
}

#[cfg(test)]
pub(crate) fn test_execution_challenge_publication_receipt(
    claim: &ExecutionChallengePublicationClaim,
    channel: &VerifiedProofChannelBinding,
    file: TestControlFile,
) -> Result<ExecutionChallengePublicationReceipt, HostError> {
    let receipt = ExecutionChallengePublicationReceipt {
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
        metadata_volume_name: channel.metadata_volume_name().to_owned(),
        metadata_volume_id: channel.metadata_volume_id().to_owned(),
        challenge_digest: claim.challenge_digest().to_owned(),
        file: control_file(file),
    };
    let identity = CommonReceiptIdentity {
        connection_id: &receipt.connection_id,
        reservation_id: receipt.reservation_id,
        operation_id: receipt.operation_id,
        scale_set_id: receipt.scale_set_id,
        witness_request_id: receipt.witness_request_id,
        engine_id: &receipt.engine_id,
        docker_root_digest: &receipt.docker_root_digest,
        runner_id: receipt.runner_id,
        runner_name: &receipt.runner_name,
        runner_container_id: &receipt.runner_container_id,
        dind_container_id: &receipt.dind_container_id,
        instance_binding_digest: &receipt.instance_binding_digest,
        challenge: &receipt.challenge,
        policy_generation: receipt.policy_generation,
        policy_digest: &receipt.policy_digest,
        channel_digest: &receipt.channel_digest,
    };
    if !valid_common(&identity)
        || !valid_volume_identity(&receipt.metadata_volume_name, &receipt.metadata_volume_id)
        || !valid_digest(&receipt.challenge_digest)
        || !validate_control_file(&receipt.file, CHALLENGE_PATH)
    {
        return Err(HostError::Journal);
    }
    Ok(receipt)
}

#[cfg(test)]
fn control_file(file: TestControlFile) -> ControlFileReceipt {
    ControlFileReceipt {
        path: file.path,
        inode: file.inode,
        bytes: file.bytes,
        control_directory_inode: file.control_directory_inode,
        uid: file.uid,
        gid: file.gid,
        mode: file.mode,
        mount_id: file.mount_id,
        device_major: file.device.0,
        device_minor: file.device.1,
        statx_device_major: file.statx_device.0,
        statx_device_minor: file.statx_device.1,
        fs_type_magic: file.fs_type_magic,
        filesystem_id: file.filesystem_id,
        filesystem_instance_digest: file.filesystem_instance_digest,
        mount_source: file.mount_source,
        read_only: file.read_only,
        no_copy: file.no_copy,
    }
}

fn valid_volume_identity(name: &str, id: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        && valid_object_id(id)
}

fn valid_common(identity: &CommonReceiptIdentity<'_>) -> bool {
    !identity.connection_id.is_empty()
        && identity.reservation_id > 0
        && identity.operation_id > 0
        && identity.scale_set_id > 0
        && identity.witness_request_id > 0
        && valid_engine(identity.engine_id)
        && valid_digest(identity.docker_root_digest)
        && identity.runner_id > 0
        && !identity.runner_name.is_empty()
        && valid_object_id(identity.runner_container_id)
        && valid_object_id(identity.dind_container_id)
        && identity.runner_container_id != identity.dind_container_id
        && valid_digest(identity.instance_binding_digest)
        && valid_digest(identity.challenge)
        && identity.policy_generation > 0
        && valid_digest(identity.policy_digest)
        && valid_digest(identity.channel_digest)
}

#[cfg(test)]
mod tests {
    use super::valid_volume_identity;

    #[test]
    fn publisher_receipt_requires_exact_safe_volume_identity() {
        assert!(valid_volume_identity("velnor-meta-1", "0123456789ab"));
        assert!(!valid_volume_identity("", "0123456789ab"));
        assert!(!valid_volume_identity("velnor/meta", "0123456789ab"));
        assert!(!valid_volume_identity("velnor-meta-1", "short"));
        assert!(!valid_volume_identity("velnor-meta-1", "0123456789ab\n"));
    }
}
