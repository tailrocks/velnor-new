//! Exact host-written files in the source-proof control directory.

const CHALLENGE_PATH: &str = "/opt/velnor/source-proof/control/challenge.json";
const DECISION_PATH: &str = "/opt/velnor/source-proof/control/decision.json";
const CONTROL_FILE_MODE: u32 = 0o444;
const ROOT_UID: u32 = 0;
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;
const MAX_CONTROL_FILE_BYTES: u64 = 8 * 1024;

struct ControlFileReceipt {
    path: String,
    inode: u64,
    bytes: u64,
    control_directory_inode: u64,
    uid: u32,
    gid: u32,
    mode: u32,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    fs_type_magic: u64,
    filesystem_id: u64,
    filesystem_instance_digest: String,
    mount_source: String,
    read_only: bool,
    no_copy: bool,
}

/// Receipt that the challenge file is exact, immutable, and readable by Runner.
pub(crate) struct ExecutionChallengePublicationReceipt {
    reservation_id: i64,
    operation_id: i64,
    connection_id: String,
    scale_set_id: i64,
    witness_request_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_id: i64,
    runner_name: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    challenge: String,
    policy_generation: u64,
    policy_digest: String,
    channel_digest: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    challenge_digest: String,
    file: ControlFileReceipt,
}

impl ExecutionChallengePublicationReceipt {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) const fn witness_request_id(&self) -> i64 {
        self.witness_request_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) const fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(crate) fn runner_name(&self) -> &str {
        &self.runner_name
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn challenge(&self) -> &str {
        &self.challenge
    }

    pub(crate) const fn policy_generation(&self) -> u64 {
        self.policy_generation
    }

    pub(crate) fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    pub(crate) fn channel_digest(&self) -> &str {
        &self.channel_digest
    }

    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }

    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }

    pub(crate) const fn control_directory_inode(&self) -> u64 {
        self.file.control_directory_inode
    }

    pub(crate) fn mount_source(&self) -> &str {
        &self.file.mount_source
    }

    pub(crate) fn challenge_digest(&self) -> &str {
        &self.challenge_digest
    }

    pub(crate) fn path(&self) -> &str {
        &self.file.path
    }

    pub(crate) const fn file_identity(&self) -> (u64, u64) {
        (self.file.inode, self.file.bytes)
    }

    pub(crate) const fn file_owner(&self) -> (u32, u32) {
        (self.file.uid, self.file.gid)
    }

    pub(crate) const fn file_mode(&self) -> u32 {
        self.file.mode
    }

    pub(crate) const fn mount_id(&self) -> u64 {
        self.file.mount_id
    }

    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.file.device_major, self.file.device_minor)
    }

    pub(crate) const fn statx_device(&self) -> (u32, u32) {
        (self.file.statx_device_major, self.file.statx_device_minor)
    }

    pub(crate) const fn fs_type_magic(&self) -> u64 {
        self.file.fs_type_magic
    }

    pub(crate) const fn filesystem_id(&self) -> u64 {
        self.file.filesystem_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.file.filesystem_instance_digest
    }

    pub(crate) const fn read_only(&self) -> bool {
        self.file.read_only
    }

    pub(crate) const fn no_copy(&self) -> bool {
        self.file.no_copy
    }
}

/// Receipt that one exact allow or deny decision was written to the control directory.
pub(crate) struct ExecutionDecisionDeliveryReceipt {
    reservation_id: i64,
    operation_id: i64,
    connection_id: String,
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
    policy_generation: u64,
    policy_digest: String,
    request_digest: String,
    decision_digest: String,
    channel_digest: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    file: ControlFileReceipt,
}

impl ExecutionDecisionDeliveryReceipt {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) const fn witness_request_id(&self) -> i64 {
        self.witness_request_id
    }

    pub(crate) const fn actual_request_id(&self) -> i64 {
        self.actual_request_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) const fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(crate) fn runner_name(&self) -> &str {
        &self.runner_name
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn challenge(&self) -> &str {
        &self.challenge
    }

    pub(crate) const fn policy_generation(&self) -> u64 {
        self.policy_generation
    }

    pub(crate) fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    pub(crate) fn request_digest(&self) -> &str {
        &self.request_digest
    }

    pub(crate) fn decision_digest(&self) -> &str {
        &self.decision_digest
    }

    pub(crate) fn channel_digest(&self) -> &str {
        &self.channel_digest
    }

    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }

    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }

    pub(crate) const fn control_directory_inode(&self) -> u64 {
        self.file.control_directory_inode
    }

    pub(crate) fn mount_source(&self) -> &str {
        &self.file.mount_source
    }

    pub(crate) fn path(&self) -> &str {
        &self.file.path
    }

    pub(crate) const fn file_identity(&self) -> (u64, u64) {
        (self.file.inode, self.file.bytes)
    }

    pub(crate) const fn file_owner(&self) -> (u32, u32) {
        (self.file.uid, self.file.gid)
    }

    pub(crate) const fn file_mode(&self) -> u32 {
        self.file.mode
    }

    pub(crate) const fn mount_id(&self) -> u64 {
        self.file.mount_id
    }

    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.file.device_major, self.file.device_minor)
    }

    pub(crate) const fn statx_device(&self) -> (u32, u32) {
        (self.file.statx_device_major, self.file.statx_device_minor)
    }

    pub(crate) const fn fs_type_magic(&self) -> u64 {
        self.file.fs_type_magic
    }

    pub(crate) const fn filesystem_id(&self) -> u64 {
        self.file.filesystem_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.file.filesystem_instance_digest
    }

    pub(crate) const fn read_only(&self) -> bool {
        self.file.read_only
    }

    pub(crate) const fn no_copy(&self) -> bool {
        self.file.no_copy
    }
}

#[cfg(test)]
mod fixtures;
#[cfg(test)]
pub(crate) use fixtures::{TestControlFile, test_execution_challenge_publication_receipt};

mod production;
pub(crate) use production::{
    VerifiedMetadataVolume, challenge_publication_receipt, decision_delivery_receipt,
    inspect_metadata_volume,
};
