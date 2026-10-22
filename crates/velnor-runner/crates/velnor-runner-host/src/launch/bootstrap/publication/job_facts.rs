use velnor_resource_probe::job_facts_wire::JobFactsPublicationRecord;
use velnor_runner_github::RunnerReference;

use crate::{
    journal::{BoundJobFactsSnapshot, JobFactsPublicationClaim},
    worker::{
        execution_authorization::VerifiedProofChannelBinding,
        storage_initialization::VerifiedStorageInitializationReady,
    },
};

mod identity;
use self::identity::{digest, lower_hex};

#[cfg(test)]
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;

/// Proof that the controller published the exact cache projection below the fixed read-only job-facts mount.
pub(crate) struct JobFactsPublication {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    profile_generation_id: String,
    profile_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    runner: RunnerReference,
    instance_binding_digest: String,
    path: String,
    projection_digest: String,
    mount_id: u64,
    directory_device_major: u32,
    directory_device_minor: u32,
    directory_inode: u64,
    filesystem_id: u64,
    filesystem_instance_digest: String,
    read_only: bool,
}

impl JobFactsPublication {
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

    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }

    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn projection_digest(&self) -> &str {
        &self.projection_digest
    }

    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }

    pub(crate) const fn directory_identity(&self) -> (u32, u32, u64) {
        (
            self.directory_device_major,
            self.directory_device_minor,
            self.directory_inode,
        )
    }

    pub(crate) const fn filesystem_id(&self) -> u64 {
        self.filesystem_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }

    pub(crate) const fn read_only(&self) -> bool {
        self.read_only
    }
}

/// Make a publication receipt from the sealed claim and measured guest record.
pub(crate) fn job_facts_publication(
    claim: &JobFactsPublicationClaim,
    snapshot: &BoundJobFactsSnapshot,
    ready: &VerifiedStorageInitializationReady,
    channel: &VerifiedProofChannelBinding,
    request: &velnor_resource_probe::job_facts_wire::JobFactsPublicationContext,
    record: &JobFactsPublicationRecord,
) -> Result<JobFactsPublication, crate::error::HostError> {
    let pair_sha256 = velnor_resource_probe::job_facts_wire::paired_payload_sha256(request)
        .map_err(|_| crate::error::HostError::Identity)?;
    let context = JobFactsPublicationValidation {
        claim,
        snapshot,
        ready,
        channel,
        request,
        record,
        pair_sha256: &pair_sha256,
        job_facts_sha256: &digest(snapshot.bytes()),
    };
    validate_claim_and_record(&context)?;
    validate_job_facts_mount(&context)?;
    validate_runtime_identity_mount(&context)?;
    validate_directory_identity(&context)?;
    let observed = &record.directory;
    JobFactsPublication::from_guest_record(
        claim,
        snapshot,
        &record.guest_boot_id,
        observed,
        &record.file,
    )
}

struct JobFactsPublicationValidation<'a> {
    claim: &'a JobFactsPublicationClaim,
    snapshot: &'a BoundJobFactsSnapshot,
    ready: &'a VerifiedStorageInitializationReady,
    channel: &'a VerifiedProofChannelBinding,
    request: &'a velnor_resource_probe::job_facts_wire::JobFactsPublicationContext,
    record: &'a JobFactsPublicationRecord,
    pair_sha256: &'a str,
    job_facts_sha256: &'a str,
}

fn validate_claim_and_record(
    value: &JobFactsPublicationValidation<'_>,
) -> Result<(), crate::error::HostError> {
    let claim = value.claim;
    let snapshot = value.snapshot;
    let ready = value.ready;
    let channel = value.channel;
    let request = value.request;
    let record = value.record;
    if !channel.valid()
        || !channel.matches_initialized_job_facts(ready)
        || snapshot.physical_binding_digest() != claim.instance_binding_digest()
        || channel.instance_binding_digest() != claim.instance_binding_digest()
        || claim.reservation_id() != ready.reservation_id()
        || claim.operation_id() != ready.operation_id()
        || claim.connection_id() != ready.connection_id()
        || claim.engine_id() != ready.engine_id()
        || claim.docker_root_digest() != ready.docker_root_digest()
        || claim.runner_container_id() != channel.runner_container_id()
        || claim.dind_container_id() != channel.dind_container_id()
        || claim.runner().id != channel.runner_id()
        || claim.runner().name != channel.runner_name()
        || claim.runner().runner_scale_set_id != channel.scale_set_id()
        || record.schema_version != 2
        || record.operation_id.len() != 32
        || !lower_hex(&record.operation_id)
        || record.guest_boot_id != channel.observation().guest_boot_id
        || record.mount_namespace_id == 0
        || record.payload_sha256 != request.payload_sha256
        || record.payload_sha256 != *value.pair_sha256
        || record.guest_image_sha256 != request.guest_image_sha256
        || record.publication_binding_sha256 != request.publication_binding_sha256
    {
        return Err(crate::error::HostError::Identity);
    }
    Ok(())
}

fn validate_job_facts_mount(
    value: &JobFactsPublicationValidation<'_>,
) -> Result<(), crate::error::HostError> {
    let observed = &value.record.directory;
    let expected = value.channel.job_facts_directory();
    let file = &value.record.file;
    if observed.target != "/opt/velnor/job-facts"
        || observed.inode == 0
        || observed.owner_uid != 0
        || observed.owner_gid != 0
        || observed.mode != 0o555
        || observed.read_only
        || observed.mount_id == 0
        || observed.statx_mount_id != observed.mount_id
        || !velnor_resource_probe::job_facts_wire::mount_root_contains_subpath(
            &observed.mount_root,
            &expected.mount_root,
            "job-facts",
        )
        || observed.mount_source != expected.mount_source
        || observed.mount_device_major != expected.mount_device_major
        || observed.mount_device_minor != expected.mount_device_minor
        || observed.statx_device_major != expected.statx_device_major
        || observed.statx_device_minor != expected.statx_device_minor
        || observed.filesystem_type_magic != expected.filesystem_type_magic
        || observed.filesystem_type_name != expected.filesystem_type_name
        || observed.filesystem_id != expected.filesystem_id
        || observed.executable_mount != expected.executable_mount
        || file.path != "/opt/velnor/job-facts/storage-cache-admission.json"
        || file.inode == 0
        || file.bytes != value.snapshot.bytes().len() as u64
        || file.owner_uid != 0
        || file.owner_gid != 0
        || file.mode != 0o444
        || file.readback_sha256 != *value.job_facts_sha256
    {
        return Err(crate::error::HostError::Identity);
    }
    Ok(())
}

fn validate_runtime_identity_mount(
    value: &JobFactsPublicationValidation<'_>,
) -> Result<(), crate::error::HostError> {
    let observed = &value.record.directory;
    let runtime = &value.record.runtime_identity_directory;
    let expected = &value.request.expected_runner_runtime_identity;
    let guest_image_file = &value.record.guest_image_file;
    if runtime.target != "/opt/velnor/runtime-identity"
        || runtime.inode != expected.inode
        || runtime.owner_uid != expected.owner_uid
        || runtime.owner_gid != expected.owner_gid
        || runtime.mode != expected.mode
        || runtime.read_only
        || runtime.mount_root != observed.mount_root
        || runtime.mount_source != observed.mount_source
        || runtime.mount_device_major != observed.mount_device_major
        || runtime.mount_device_minor != observed.mount_device_minor
        || runtime.filesystem_type_magic != observed.filesystem_type_magic
        || runtime.filesystem_type_name != observed.filesystem_type_name
        || runtime.filesystem_id != observed.filesystem_id
        || runtime.statx_device_major != expected.device_major
        || runtime.statx_device_minor != expected.device_minor
        || guest_image_file.path != "/opt/velnor/runtime-identity/guest-image.json"
        || guest_image_file.inode == 0
        || guest_image_file.bytes != value.request.guest_image_hex.len() as u64 / 2
        || guest_image_file.owner_uid != 0
        || guest_image_file.owner_gid != 0
        || guest_image_file.mode != 0o444
        || guest_image_file.readback_sha256 != value.request.guest_image_sha256
    {
        return Err(crate::error::HostError::Identity);
    }
    Ok(())
}

fn validate_directory_identity(
    value: &JobFactsPublicationValidation<'_>,
) -> Result<(), crate::error::HostError> {
    let claim = value.claim;
    let record = value.record;
    let channel = value.channel;
    let snapshot = value.snapshot;
    let expected_runtime = &value.request.expected_runner_runtime_identity;
    let observed = &record.directory;
    let directory = (
        observed.statx_device_major,
        observed.statx_device_minor,
        observed.inode,
    );
    let expected = channel.job_facts_directory();
    if directory.2 != snapshot.runner_directory_inode()
        || (
            expected.statx_device_major,
            expected.statx_device_minor,
            expected.inode,
        ) != directory
    {
        return Err(crate::error::HostError::Identity);
    }
    let runtime = &record.runtime_identity_directory;
    if (
        runtime.statx_device_major,
        runtime.statx_device_minor,
        runtime.inode,
    ) != (
        expected_runtime.device_major,
        expected_runtime.device_minor,
        expected_runtime.inode,
    ) {
        return Err(crate::error::HostError::Identity);
    }
    let measured = crate::launch::resource_probe::filesystem_instance_digest(
        &crate::launch::resource_probe::FilesystemInstanceDigestInput {
            engine_id: claim.engine_id(),
            docker_root_digest: claim.docker_root_digest(),
            guest_boot_id: record.guest_boot_id.as_bytes(),
            mount_source: observed.mount_source.as_bytes(),
            filesystem_type_name: observed.filesystem_type_name.as_bytes(),
            filesystem_type_magic: observed.filesystem_type_magic,
            mount_device_major: observed.mount_device_major,
            mount_device_minor: observed.mount_device_minor,
            statx_device_major: observed.statx_device_major,
            statx_device_minor: observed.statx_device_minor,
            filesystem_id: observed.filesystem_id,
        },
    );
    if measured != channel.job_facts().filesystem_instance_digest() {
        return Err(crate::error::HostError::Identity);
    }
    Ok(())
}
mod record;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
#[cfg(test)]
pub(crate) use tests::{JobFactsPublicationFixtureInput, test_job_facts_publication};
