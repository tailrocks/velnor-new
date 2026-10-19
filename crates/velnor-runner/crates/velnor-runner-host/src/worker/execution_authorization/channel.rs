//! Opaque receipts for the Runner source-proof channel.

use crate::error::HostError;
use crate::worker::storage_initialization::VerifiedStorageInitializationReady;
use serde::Serialize;
use velnor_resource_probe::execution_channel_wire::{
    ExecutionChannelDirectory, ExecutionChannelObservation,
};

mod measure;
pub(crate) use measure::measure_proof_channel;
mod read_request;
pub(crate) use read_request::{
    ExecutionRequestRead, VerifiedExecutionRequest, read_execution_request,
};

#[cfg(test)]
mod fixtures;
#[cfg(test)]
pub(crate) use fixtures::{
    test_channel_for_ready_job_facts, test_proof_channel_mount, test_verified_proof_channel_binding,
};

const CONTROL_TARGET: &str = "/opt/velnor/source-proof/control";
const INBOX_TARGET: &str = "/opt/velnor/source-proof/inbox";
const JOB_FACTS_TARGET: &str = "/opt/velnor/job-facts";
const CONTROL_SUBPATH: &str = "source-proof/control";
const INBOX_SUBPATH: &str = "source-proof/inbox";
const JOB_FACTS_SUBPATH: &str = "job-facts";
const RUNNER_UID: u32 = 1000;
const ROOT_UID: u32 = 0;
const DIRECTORY_MODE: u32 = 0o555;
const INBOX_MODE: u32 = 0o700;

/// One measured mount and directory identity in the source-proof channel.
#[derive(Serialize)]
pub(crate) struct ProofChannelMount {
    target: String,
    subpath: String,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    fs_type_magic: u64,
    filesystem_id: u64,
    filesystem_instance_digest: String,
    directory_inode: u64,
    owner_uid: u32,
    owner_gid: u32,
    mode: u32,
    read_only: bool,
    no_copy: bool,
}

/// Evidence that one Runner has the fixed source-proof directories.
///
/// The control directory is read-only. The inbox is writable by the Runner
/// account. The proof does not grant access to a job container or `DinD`.
pub(crate) struct VerifiedProofChannelBinding {
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
    metadata_volume_name: String,
    metadata_volume_id: String,
    control: ProofChannelMount,
    control_directory: ExecutionChannelDirectory,
    inbox: ProofChannelMount,
    job_facts: ProofChannelMount,
    observation: ExecutionChannelObservation,
    dind_attachment_absent: bool,
    channel_digest: String,
}

impl VerifiedProofChannelBinding {
    /// Encode the exact measured channel facts without the derived digest.
    pub(crate) fn canonical_json(&self) -> Result<Vec<u8>, HostError> {
        serde_json::to_vec(&ChannelContent {
            schema_version: 3,
            reservation_id: self.reservation_id,
            operation_id: self.operation_id,
            connection_id: &self.connection_id,
            scale_set_id: self.scale_set_id,
            witness_request_id: self.witness_request_id,
            engine_id: &self.engine_id,
            docker_root_digest: &self.docker_root_digest,
            runner_id: self.runner_id,
            runner_name: &self.runner_name,
            runner_container_id: &self.runner_container_id,
            dind_container_id: &self.dind_container_id,
            instance_binding_digest: &self.instance_binding_digest,
            metadata_volume_name: &self.metadata_volume_name,
            metadata_volume_id: &self.metadata_volume_id,
            control: &self.control,
            inbox: &self.inbox,
            job_facts: &self.job_facts,
            observation: &self.observation,
            dind_attachment_absent: self.dind_attachment_absent,
        })
        .map_err(|_| HostError::Journal)
    }

    pub(crate) fn computed_digest(&self) -> Result<String, HostError> {
        use sha2::Digest as _;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut digest = String::with_capacity(64);
        for byte in sha2::Sha256::digest(self.canonical_json()?) {
            digest.push(char::from(HEX[usize::from(byte >> 4)]));
            digest.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Ok(digest)
    }

    /// Recheck the fixed channel shape and its canonical identity before persistence.
    pub(crate) fn valid(&self) -> bool {
        self.reservation_id > 0
            && self.operation_id > 0
            && !self.connection_id.is_empty()
            && self.scale_set_id > 0
            && self.witness_request_id > 0
            && valid_engine(&self.engine_id)
            && valid_digest(&self.docker_root_digest)
            && self.runner_id > 0
            && !self.runner_name.is_empty()
            && valid_object_id(&self.runner_container_id)
            && valid_object_id(&self.dind_container_id)
            && self.runner_container_id != self.dind_container_id
            && valid_digest(&self.instance_binding_digest)
            && valid_object_id(&self.metadata_volume_id)
            && valid_volume_name(&self.metadata_volume_name)
            && valid_control_mount(&self.control)
            && valid_inbox_mount(&self.inbox)
            && valid_job_facts_mount(&self.job_facts)
            && self.observation.schema_version == 3
            && valid_operation_id(&self.observation.operation_id)
            && !self.observation.guest_boot_id.is_empty()
            && self.observation.mount_namespace_id > 0
            && self.observation.control == self.control_directory
            && !self.observation.control.mount_source.is_empty()
            && !self.observation.control.filesystem_type_name.is_empty()
            && !self.observation.control.executable_mount
            && self.observation.inbox.mount_source == "tmpfs"
            && self.observation.inbox.filesystem_type_name == "tmpfs"
            && !self.observation.inbox.executable_mount
            && !self.observation.job_facts.mount_source.is_empty()
            && !self.observation.job_facts.filesystem_type_name.is_empty()
            && self.observation.job_facts.read_only
            && directory_matches_mount(&self.observation.control, &self.control)
            && directory_matches_mount(&self.observation.inbox, &self.inbox)
            && directory_matches_mount(&self.observation.job_facts, &self.job_facts)
            && self.control.mount_id != self.inbox.mount_id
            && self.job_facts.mount_id != self.inbox.mount_id
            && self.job_facts.target != self.control.target
            && self.control.directory_inode != self.inbox.directory_inode
            && self.job_facts.directory_inode != self.inbox.directory_inode
            && self.job_facts.directory_inode != self.control.directory_inode
            && self.dind_attachment_absent
            && self
                .computed_digest()
                .is_ok_and(|value| value == self.channel_digest)
    }

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

    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }

    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }

    pub(crate) fn control(&self) -> &ProofChannelMount {
        &self.control
    }

    pub(crate) fn control_directory(&self) -> &ExecutionChannelDirectory {
        &self.control_directory
    }

    pub(crate) fn inbox(&self) -> &ProofChannelMount {
        &self.inbox
    }

    pub(crate) fn job_facts(&self) -> &ProofChannelMount {
        &self.job_facts
    }

    pub(crate) fn job_facts_directory(&self) -> &ExecutionChannelDirectory {
        &self.observation.job_facts
    }

    /// Match Runner facts to the independently measured initialized source directory.
    pub(crate) fn matches_initialized_job_facts(
        &self,
        ready: &VerifiedStorageInitializationReady,
    ) -> bool {
        let source = ready.job_facts();
        let runner = &self.observation.job_facts;
        ready.connection_id() == self.connection_id
            && ready.reservation_id() == self.reservation_id
            && ready.operation_id() == self.operation_id
            && ready.engine_id() == self.engine_id
            && ready.docker_root_digest() == self.docker_root_digest
            && ready.metadata_volume_name() == self.metadata_volume_name
            && ready.metadata_volume_id() == self.metadata_volume_id
            && ready.layout_version() == 2
            && ready.layout_digest().len() == 64
            && source.inode() == runner.inode
            && linux_dev_t(source.device().0, source.device().1)
                == linux_dev_t(runner.statx_device_major, runner.statx_device_minor)
            && source.owner() == (runner.owner_uid, runner.owner_gid)
            && source.mode() & 0o7777 == runner.mode
            && runner.target == JOB_FACTS_TARGET
            && runner.read_only
    }

    pub(crate) fn observation(&self) -> &ExecutionChannelObservation {
        &self.observation
    }

    pub(crate) fn channel_digest(&self) -> &str {
        &self.channel_digest
    }

    pub(crate) const fn dind_attachment_absent(&self) -> bool {
        self.dind_attachment_absent
    }
}

fn linux_dev_t(major: u32, minor: u32) -> u64 {
    (u64::from(major & 0x0fff) << 8)
        | (u64::from(major & 0xffff_f000) << 32)
        | u64::from(minor & 0x00ff)
        | (u64::from(minor & 0xffff_ff00) << 12)
}

#[cfg(test)]
mod linux_device_tests {
    use super::linux_dev_t;

    #[test]
    fn linux_device_encoding_preserves_extended_minor_and_major_bits() {
        assert_eq!(linux_dev_t(0, 48), 48);
        assert_eq!(linux_dev_t(0, 256), 1_048_576);
        assert_eq!(linux_dev_t(0x1234_5678, 0x9abc_def0), 0x1234_59ab_cde6_78f0);
    }
}

#[derive(Serialize)]
struct ChannelContent<'a> {
    schema_version: u32,
    reservation_id: i64,
    operation_id: i64,
    connection_id: &'a str,
    scale_set_id: i64,
    witness_request_id: i64,
    engine_id: &'a str,
    docker_root_digest: &'a str,
    runner_id: i64,
    runner_name: &'a str,
    runner_container_id: &'a str,
    dind_container_id: &'a str,
    instance_binding_digest: &'a str,
    metadata_volume_name: &'a str,
    metadata_volume_id: &'a str,
    control: &'a ProofChannelMount,
    inbox: &'a ProofChannelMount,
    job_facts: &'a ProofChannelMount,
    observation: &'a ExecutionChannelObservation,
    dind_attachment_absent: bool,
}

impl ProofChannelMount {
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn subpath(&self) -> &str {
        &self.subpath
    }

    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }

    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.device_major, self.device_minor)
    }

    pub(crate) const fn statx_device(&self) -> (u32, u32) {
        (self.statx_device_major, self.statx_device_minor)
    }

    pub(crate) const fn fs_type_magic(&self) -> u64 {
        self.fs_type_magic
    }

    pub(crate) const fn filesystem_id(&self) -> u64 {
        self.filesystem_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }

    pub(crate) const fn directory_inode(&self) -> u64 {
        self.directory_inode
    }

    pub(crate) const fn owner(&self) -> (u32, u32) {
        (self.owner_uid, self.owner_gid)
    }

    pub(crate) const fn mode(&self) -> u32 {
        self.mode
    }

    pub(crate) const fn read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) const fn no_copy(&self) -> bool {
        self.no_copy
    }
}

pub(super) fn valid_control_mount(mount: &ProofChannelMount) -> bool {
    mount.target == CONTROL_TARGET
        && mount.subpath == CONTROL_SUBPATH
        && mount.mount_id > 0
        && mount.fs_type_magic > 0
        && mount.filesystem_id > 0
        && mount.directory_inode > 0
        && valid_digest(&mount.filesystem_instance_digest)
        && mount.owner_uid == ROOT_UID
        && mount.owner_gid == ROOT_UID
        && mount.mode == DIRECTORY_MODE
        && mount.read_only
        && mount.no_copy
        && same_device_or_btrfs(mount)
}

pub(super) fn valid_inbox_mount(mount: &ProofChannelMount) -> bool {
    const TMPFS_MAGIC: u64 = 0x0102_1994;
    mount.target == INBOX_TARGET
        && mount.subpath == INBOX_SUBPATH
        && mount.mount_id > 0
        && mount.fs_type_magic > 0
        && mount.filesystem_id > 0
        && mount.directory_inode > 0
        && valid_digest(&mount.filesystem_instance_digest)
        && mount.owner_uid == RUNNER_UID
        && mount.owner_gid == RUNNER_UID
        && mount.mode == INBOX_MODE
        && mount.fs_type_magic == TMPFS_MAGIC
        && !mount.read_only
        && mount.no_copy
        && same_device_or_btrfs(mount)
}

pub(super) fn valid_job_facts_mount(mount: &ProofChannelMount) -> bool {
    mount.target == JOB_FACTS_TARGET
        && mount.subpath == JOB_FACTS_SUBPATH
        && mount.mount_id > 0
        && mount.fs_type_magic > 0
        && mount.filesystem_id > 0
        && mount.directory_inode > 0
        && valid_digest(&mount.filesystem_instance_digest)
        && mount.owner_uid == ROOT_UID
        && mount.owner_gid == ROOT_UID
        && mount.mode == DIRECTORY_MODE
        && mount.read_only
        && mount.no_copy
        && same_device_or_btrfs(mount)
}

pub(super) fn same_device_or_btrfs(mount: &ProofChannelMount) -> bool {
    (mount.device_major, mount.device_minor) == (mount.statx_device_major, mount.statx_device_minor)
        || mount.fs_type_magic == 0x9123_683e
}

pub(super) fn valid_engine(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

pub(super) fn valid_object_id(value: &str) -> bool {
    (12..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn valid_operation_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn directory_matches_mount(
    directory: &ExecutionChannelDirectory,
    mount: &ProofChannelMount,
) -> bool {
    directory.target == mount.target
        && directory.inode == mount.directory_inode
        && directory.owner_uid == mount.owner_uid
        && directory.owner_gid == mount.owner_gid
        && directory.mode == mount.mode
        && directory.mount_id == mount.mount_id
        && directory.mount_device_major == mount.device_major
        && directory.mount_device_minor == mount.device_minor
        && directory.statx_device_major == mount.statx_device_major
        && directory.statx_device_minor == mount.statx_device_minor
        && directory.filesystem_type_magic == mount.fs_type_magic
        && directory.filesystem_id == mount.filesystem_id
        && directory.read_only == mount.read_only
}

pub(super) fn valid_volume_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
