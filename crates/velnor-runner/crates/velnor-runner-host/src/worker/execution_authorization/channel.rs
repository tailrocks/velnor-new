//! Opaque receipts for the Runner source-proof channel.

use crate::error::HostError;
use crate::worker::storage_initialization::VerifiedStorageInitializationReady;
use serde::Serialize;
use velnor_resource_probe::execution_channel_wire::{
    ExecutionChannelDirectory, ExecutionChannelObservation,
};

mod measure;
pub(crate) use measure::{ProofChannelMeasurement, measure_proof_channel};
mod read_request;
pub(crate) use read_request::{
    ExecutionRequestRead, VerifiedExecutionRequest, read_execution_request,
};

#[cfg(test)]
mod fixtures;
#[cfg(test)]
pub(crate) use fixtures::{
    TestProofChannelBindingFields, TestProofChannelIdentity, TestProofChannelMount,
    test_channel_for_ready_job_facts, test_proof_channel_mount,
    test_verified_proof_channel_binding,
};

mod validation;
pub(super) use validation::{
    JOB_FACTS_TARGET, valid_control_mount, valid_digest, valid_engine, valid_inbox_mount,
    valid_job_facts_mount, valid_object_id, valid_volume_name,
};
use validation::{directory_matches_mount, valid_operation_id};

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
    #[cfg(test)]
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    #[cfg(test)]
    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }

    #[cfg(test)]
    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.device_major, self.device_minor)
    }

    #[cfg(test)]
    pub(crate) const fn statx_device(&self) -> (u32, u32) {
        (self.statx_device_major, self.statx_device_minor)
    }

    #[cfg(test)]
    pub(crate) const fn fs_type_magic(&self) -> u64 {
        self.fs_type_magic
    }

    #[cfg(test)]
    pub(crate) const fn filesystem_id(&self) -> u64 {
        self.filesystem_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }

    #[cfg(test)]
    pub(crate) const fn directory_inode(&self) -> u64 {
        self.directory_inode
    }

    #[cfg(test)]
    pub(crate) const fn owner(&self) -> (u32, u32) {
        (self.owner_uid, self.owner_gid)
    }

    #[cfg(test)]
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
