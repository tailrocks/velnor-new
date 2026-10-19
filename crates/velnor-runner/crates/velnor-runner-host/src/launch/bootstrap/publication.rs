use velnor_resource_probe::job_facts_wire::JobFactsPublicationRecord;
use velnor_runner_github::RunnerReference;

use crate::{
    journal::{BoundJobFactsSnapshot, JobFactsPublicationClaim},
    worker::{
        execution_authorization::VerifiedProofChannelBinding,
        storage_initialization::VerifiedStorageInitializationReady,
    },
};

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
    device_major: u32,
    device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    fs_type_magic: u64,
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

    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.device_major, self.device_minor)
    }

    pub(crate) const fn statx_device(&self) -> (u32, u32) {
        (self.statx_device_major, self.statx_device_minor)
    }

    pub(crate) const fn fs_type_magic(&self) -> u64 {
        self.fs_type_magic
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
    record: JobFactsPublicationRecord,
) -> Result<JobFactsPublication, crate::error::HostError> {
    let expected = channel.job_facts_directory();
    let observed = &record.directory;
    let file = &record.file;
    let payload_sha256 = digest(snapshot.bytes());
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
        || record.schema_version != 1
        || record.operation_id.len() != 32
        || !lower_hex(&record.operation_id)
        || record.guest_boot_id != channel.observation().guest_boot_id
        || record.mount_namespace_id == 0
        || record.payload_sha256 != payload_sha256
        || observed.target != "/opt/velnor/job-facts"
        || observed.inode == 0
        || observed.owner_uid != 0
        || observed.owner_gid != 0
        || observed.mode != 0o555
        || observed.read_only
        || observed.mount_id == 0
        || observed.statx_mount_id != observed.mount_id
        || observed.mount_root != expected.mount_root
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
        || file.bytes != snapshot.bytes().len() as u64
        || file.owner_uid != 0
        || file.owner_gid != 0
        || file.mode != 0o444
        || file.readback_sha256 != payload_sha256
    {
        return Err(crate::error::HostError::Identity);
    }
    let directory = (
        observed.statx_device_major,
        observed.statx_device_minor,
        observed.inode,
    );
    if directory.2 != snapshot.runner_directory_inode()
        || (
            expected.statx_device_major,
            expected.statx_device_minor,
            expected.inode,
        ) != directory
    {
        return Err(crate::error::HostError::Identity);
    }
    let measured_filesystem = crate::launch::resource_probe::filesystem_instance_digest(
        claim.engine_id(),
        claim.docker_root_digest(),
        record.guest_boot_id.as_bytes(),
        observed.mount_source.as_bytes(),
        observed.filesystem_type_name.as_bytes(),
        observed.filesystem_type_magic,
        observed.mount_device_major,
        observed.mount_device_minor,
        observed.statx_device_major,
        observed.statx_device_minor,
        observed.filesystem_id,
    );
    if measured_filesystem != channel.job_facts().filesystem_instance_digest() {
        return Err(crate::error::HostError::Identity);
    }
    JobFactsPublication::from_guest_record(claim, snapshot, &record.guest_boot_id, observed, file)
}

impl JobFactsPublication {
    fn from_guest_record(
        claim: &JobFactsPublicationClaim,
        snapshot: &BoundJobFactsSnapshot,
        guest_boot_id: &str,
        directory: &velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory,
        file: &velnor_resource_probe::job_facts_wire::JobFactsPublicationFile,
    ) -> Result<Self, crate::error::HostError> {
        let filesystem_instance_digest = crate::launch::resource_probe::filesystem_instance_digest(
            claim.engine_id(),
            claim.docker_root_digest(),
            guest_boot_id.as_bytes(),
            directory.mount_source.as_bytes(),
            directory.filesystem_type_name.as_bytes(),
            directory.filesystem_type_magic,
            directory.mount_device_major,
            directory.mount_device_minor,
            directory.statx_device_major,
            directory.statx_device_minor,
            directory.filesystem_id,
        );
        let value = Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            sample_generation_id: claim.sample_generation_id().to_owned(),
            profile_generation_id: claim.profile_generation_id().to_owned(),
            profile_digest: claim.profile_digest().to_owned(),
            runner_container_id: claim.runner_container_id().to_owned(),
            dind_container_id: claim.dind_container_id().to_owned(),
            runner: claim.runner().clone(),
            instance_binding_digest: claim.instance_binding_digest().to_owned(),
            path: file.path.clone(),
            projection_digest: snapshot.projection_digest().to_owned(),
            mount_id: directory.mount_id,
            device_major: directory.mount_device_major,
            device_minor: directory.mount_device_minor,
            statx_device_major: directory.statx_device_major,
            statx_device_minor: directory.statx_device_minor,
            fs_type_magic: directory.filesystem_type_magic,
            directory_device_major: directory.statx_device_major,
            directory_device_minor: directory.statx_device_minor,
            directory_inode: directory.inode,
            filesystem_id: directory.filesystem_id,
            filesystem_instance_digest,
            read_only: true,
        };
        if value.path != "/opt/velnor/job-facts/storage-cache-admission.json"
            || value.mount_id == 0
            || value.directory_inode == 0
            || value.filesystem_id == 0
            || value.filesystem_instance_digest.len() != 64
        {
            return Err(crate::error::HostError::Identity);
        }
        Ok(value)
    }
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

fn lower_hex(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn same_runner(left: &RunnerReference, right: &RunnerReference) -> bool {
    left.id == right.id
        && left.name == right.name
        && left.runner_scale_set_id == right.runner_scale_set_id
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::test_job_facts_publication;
