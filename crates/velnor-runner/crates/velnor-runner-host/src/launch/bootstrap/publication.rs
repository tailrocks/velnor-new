use velnor_runner_github::RunnerReference;

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

#[cfg(test)]
pub(crate) fn test_job_facts_publication(
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
    device: (u32, u32),
    statx_device: (u32, u32),
    fs_type_magic: u64,
    directory_identity: (u32, u32, u64),
    filesystem_id: u64,
    filesystem_instance_digest: String,
    read_only: bool,
) -> Result<JobFactsPublication, crate::error::HostError> {
    use crate::launch::bootstrap::test_fixtures::{
        valid_container_id, valid_digest, valid_engine_id, valid_generation,
    };

    if reservation_id <= 0
        || operation_id <= 0
        || !valid_engine_id(&engine_id)
        || !valid_digest(&docker_root_digest)
        || !valid_generation(&sample_generation_id)
        || !valid_generation(&profile_generation_id)
        || !valid_digest(&profile_digest)
        || !valid_container_id(&runner_container_id)
        || !valid_container_id(&dind_container_id)
        || runner_container_id == dind_container_id
        || !valid_runner(&runner)
        || !valid_digest(&instance_binding_digest)
        || path != "/opt/velnor/job-facts/storage-cache-admission.json"
        || !valid_digest(&projection_digest)
        || mount_id == 0
        || fs_type_magic == 0
        || filesystem_id == 0
        || !valid_digest(&filesystem_instance_digest)
        || !read_only
        || directory_identity.2 == 0
        || (directory_identity.0, directory_identity.1) != statx_device
        || (device != statx_device && fs_type_magic != BTRFS_SUPER_MAGIC)
    {
        return Err(crate::error::HostError::Journal);
    }
    Ok(JobFactsPublication {
        reservation_id,
        operation_id,
        engine_id,
        docker_root_digest,
        sample_generation_id,
        profile_generation_id,
        profile_digest,
        runner_container_id,
        dind_container_id,
        runner,
        instance_binding_digest,
        path,
        projection_digest,
        mount_id,
        device_major: device.0,
        device_minor: device.1,
        statx_device_major: statx_device.0,
        statx_device_minor: statx_device.1,
        fs_type_magic,
        directory_device_major: directory_identity.0,
        directory_device_minor: directory_identity.1,
        directory_inode: directory_identity.2,
        filesystem_id,
        filesystem_instance_digest,
        read_only,
    })
}

#[cfg(test)]
fn valid_runner(runner: &RunnerReference) -> bool {
    runner.id > 0
        && runner.runner_scale_set_id > 0
        && runner.name.len() == 33
        && runner.name.starts_with('v')
        && runner.name[1..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
