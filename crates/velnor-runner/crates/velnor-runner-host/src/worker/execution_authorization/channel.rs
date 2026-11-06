// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

mod validation;
pub(super) use validation::{
    JOB_FACTS_TARGET, valid_control_mount, valid_digest, valid_engine, valid_inbox_mount,
    valid_job_facts_mount, valid_object_id, valid_volume_name,
};
use validation::{directory_matches_mount, valid_operation_id};

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ProofChannelMount {
    pub target: String,
    pub subpath: String,
    pub mount_id: u64,
    pub device_major: u32,
    pub device_minor: u32,
    pub statx_device_major: u32,
    pub statx_device_minor: u32,
    pub fs_type_magic: u64,
    pub filesystem_id: u64,
    pub filesystem_instance_digest: String,
    pub directory_inode: u64,
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub mode: u32,
    pub read_only: bool,
    pub no_copy: bool,
}

#[derive(Debug)]
pub struct ProofChannelBindingInput {
    pub reservation_id: i64,
    pub operation_id: i64,
    pub connection_id: String,
    pub scale_set_id: i64,
    pub witness_request_id: i64,
    pub engine_id: String,
    pub docker_root_digest: String,
    pub runner_id: i64,
    pub runner_name: String,
    pub runner_container_id: String,
    pub dind_container_id: String,
    pub instance_binding_digest: String,
    pub metadata_volume_name: String,
    pub metadata_volume_id: String,
    pub control: ProofChannelMount,
    pub inbox: ProofChannelMount,
    pub job_facts: ProofChannelMount,
    pub observation: ExecutionChannelObservation,
    pub dind_attachment_absent: bool,
}

#[derive(Debug)]
pub struct VerifiedProofChannelBinding {
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
            && self.observation.inbox.mount_source == "tmpfs"
            && self.observation.inbox.filesystem_type_name == "tmpfs"
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

    #[must_use]
    pub fn channel_digest(&self) -> &str {
        &self.channel_digest
    }

    pub fn from_measured(input: ProofChannelBindingInput) -> Result<Self, HostError> {
        let mut binding = Self {
            reservation_id: input.reservation_id,
            operation_id: input.operation_id,
            connection_id: input.connection_id,
            scale_set_id: input.scale_set_id,
            witness_request_id: input.witness_request_id,
            engine_id: input.engine_id,
            docker_root_digest: input.docker_root_digest,
            runner_id: input.runner_id,
            runner_name: input.runner_name,
            runner_container_id: input.runner_container_id,
            dind_container_id: input.dind_container_id,
            instance_binding_digest: input.instance_binding_digest,
            metadata_volume_name: input.metadata_volume_name,
            metadata_volume_id: input.metadata_volume_id,
            control_directory: input.observation.control.clone(),
            control: input.control,
            inbox: input.inbox,
            job_facts: input.job_facts,
            observation: input.observation,
            dind_attachment_absent: input.dind_attachment_absent,
            channel_digest: String::new(),
        };
        binding.channel_digest = binding.computed_digest()?;
        if binding.valid() {
            Ok(binding)
        } else {
            Err(HostError::Identity)
        }
    }
}

fn linux_dev_t(major: u32, minor: u32) -> u64 {
    (u64::from(major & 0x0fff) << 8)
        | (u64::from(major & 0xffff_f000) << 32)
        | u64::from(minor & 0x00ff)
        | (u64::from(minor & 0xffff_ff00) << 12)
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
    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }

    pub(crate) const fn read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) const fn no_copy(&self) -> bool {
        self.no_copy
    }
}
