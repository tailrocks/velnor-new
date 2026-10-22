use super::JobFactsPublication;
use crate::journal::{BoundJobFactsSnapshot, JobFactsPublicationClaim};

impl JobFactsPublication {
    pub(super) fn from_guest_record(
        claim: &JobFactsPublicationClaim,
        snapshot: &BoundJobFactsSnapshot,
        guest_boot_id: &str,
        directory: &velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory,
        file: &velnor_resource_probe::job_facts_wire::JobFactsPublicationFile,
    ) -> Result<Self, crate::error::HostError> {
        let filesystem_instance_digest = crate::launch::resource_probe::filesystem_instance_digest(
            crate::launch::resource_probe::FilesystemInstanceDigestInput {
                engine_id: claim.engine_id(),
                docker_root_digest: claim.docker_root_digest(),
                guest_boot_id: guest_boot_id.as_bytes(),
                mount_source: directory.mount_source.as_bytes(),
                filesystem_type_name: directory.filesystem_type_name.as_bytes(),
                filesystem_type_magic: directory.filesystem_type_magic,
                mount_device_major: directory.mount_device_major,
                mount_device_minor: directory.mount_device_minor,
                statx_device_major: directory.statx_device_major,
                statx_device_minor: directory.statx_device_minor,
                filesystem_id: directory.filesystem_id,
            },
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
