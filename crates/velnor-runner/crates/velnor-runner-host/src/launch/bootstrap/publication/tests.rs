use super::*;

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
