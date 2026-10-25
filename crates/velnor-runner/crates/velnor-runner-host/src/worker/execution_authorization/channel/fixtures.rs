use super::{
    ProofChannelMount, VerifiedProofChannelBinding, valid_control_mount, valid_inbox_mount,
    valid_job_facts_mount,
};
use crate::error::HostError;
use crate::worker::storage_initialization::VerifiedStorageInitializationReady;
use velnor_resource_probe::execution_channel_wire::{
    ExecutionChannelDirectory, ExecutionChannelObservation,
};

pub(crate) struct TestProofChannelIdentity {
    pub(crate) reservation_id: i64,
    pub(crate) operation_id: i64,
    pub(crate) connection_id: String,
    pub(crate) scale_set_id: i64,
    pub(crate) witness_request_id: i64,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) runner_id: i64,
    pub(crate) runner_name: String,
    pub(crate) runner_container_id: String,
    pub(crate) dind_container_id: String,
    pub(crate) instance_binding_digest: String,
}

pub(crate) struct TestProofChannelBindingFields {
    pub(crate) identity: TestProofChannelIdentity,
    pub(crate) metadata_volume_name: String,
    pub(crate) metadata_volume_id: String,
    pub(crate) control: ProofChannelMount,
    pub(crate) inbox: ProofChannelMount,
    pub(crate) dind_attachment_absent: bool,
}

pub(crate) struct TestProofChannelMount {
    pub(crate) target: String,
    pub(crate) subpath: String,
    pub(crate) mount_id: u64,
    pub(crate) device: (u32, u32),
    pub(crate) statx_device: (u32, u32),
    pub(crate) fs_type_magic: u64,
    pub(crate) filesystem_id: u64,
    pub(crate) filesystem_instance_digest: String,
    pub(crate) directory_inode: u64,
    pub(crate) owner: (u32, u32),
    pub(crate) mode: u32,
    pub(crate) read_only: bool,
    pub(crate) no_copy: bool,
}

pub(crate) fn test_verified_proof_channel_binding(
    fields: TestProofChannelBindingFields,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let TestProofChannelBindingFields {
        identity,
        metadata_volume_name,
        metadata_volume_id,
        control,
        inbox,
        dind_attachment_absent,
    } = fields;
    let TestProofChannelIdentity {
        reservation_id,
        operation_id,
        connection_id,
        scale_set_id,
        witness_request_id,
        engine_id,
        docker_root_digest,
        runner_id,
        runner_name,
        runner_container_id,
        dind_container_id,
        instance_binding_digest,
    } = identity;
    let control_directory =
        directory_from_mount(&control, "/source-proof/control", "/dev/metadata", "ext4");
    let inbox_directory = directory_from_mount(&inbox, "/source-proof/inbox", "tmpfs", "tmpfs");
    let job_facts = test_proof_channel_mount(TestProofChannelMount {
        target: "/opt/velnor/job-facts".to_owned(),
        subpath: "job-facts".to_owned(),
        mount_id: 23,
        device: (8, 1),
        statx_device: (8, 1),
        fs_type_magic: 0xef53,
        filesystem_id: 33,
        filesystem_instance_digest: "d".repeat(64),
        directory_inode: 103,
        owner: (0, 0),
        mode: 0o555,
        read_only: true,
        no_copy: true,
    })?;
    let job_facts_directory =
        directory_from_mount(&job_facts, "/volume/job-facts", "/dev/metadata", "ext4");
    let mut binding = VerifiedProofChannelBinding {
        reservation_id,
        operation_id,
        connection_id,
        scale_set_id,
        witness_request_id,
        engine_id,
        docker_root_digest,
        runner_id,
        runner_name,
        runner_container_id,
        dind_container_id,
        instance_binding_digest,
        metadata_volume_name,
        metadata_volume_id,
        control_directory: control_directory.clone(),
        control,
        inbox,
        job_facts,
        observation: ExecutionChannelObservation {
            schema_version: 3,
            operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
            guest_boot_id: "11111111-2222-4333-8444-555555555555".to_owned(),
            mount_namespace_id: 44,
            control: control_directory,
            inbox: inbox_directory,
            job_facts: job_facts_directory,
        },
        dind_attachment_absent,
        channel_digest: String::new(),
    };
    binding.channel_digest = binding.computed_digest()?;
    if binding.valid() {
        Ok(binding)
    } else {
        Err(HostError::Journal)
    }
}

/// Make a test channel use the `JobFacts` directory from the verified storage result.
pub(crate) fn test_channel_for_ready_job_facts(
    identity: TestProofChannelIdentity,
    ready: &VerifiedStorageInitializationReady,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let mut binding = test_verified_proof_channel_binding(TestProofChannelBindingFields {
        identity,
        metadata_volume_name: ready.metadata_volume_name().to_owned(),
        metadata_volume_id: ready.metadata_volume_id().to_owned(),
        control: test_proof_channel_mount(TestProofChannelMount {
            target: "/opt/velnor/source-proof/control".to_owned(),
            subpath: "source-proof/control".to_owned(),
            mount_id: 21,
            device: (8, 1),
            statx_device: (8, 1),
            fs_type_magic: 0xef53,
            filesystem_id: 31,
            filesystem_instance_digest: "c".repeat(64),
            directory_inode: 101,
            owner: (0, 0),
            mode: 0o555,
            read_only: true,
            no_copy: true,
        })?,
        inbox: test_proof_channel_mount(TestProofChannelMount {
            target: "/opt/velnor/source-proof/inbox".to_owned(),
            subpath: "source-proof/inbox".to_owned(),
            mount_id: 22,
            device: (0, 77),
            statx_device: (0, 77),
            fs_type_magic: 0x0102_1994,
            filesystem_id: 32,
            filesystem_instance_digest: "d".repeat(64),
            directory_inode: 102,
            owner: (1000, 1000),
            mode: 0o700,
            read_only: false,
            no_copy: true,
        })?,
        dind_attachment_absent: true,
    })?;

    let source = ready.job_facts();
    let device = source.device();
    let job_facts = test_proof_channel_mount(TestProofChannelMount {
        target: "/opt/velnor/job-facts".to_owned(),
        subpath: "job-facts".to_owned(),
        mount_id: 23,
        device,
        statx_device: device,
        fs_type_magic: 0xef53,
        filesystem_id: 33,
        filesystem_instance_digest: "e".repeat(64),
        directory_inode: source.inode(),
        owner: source.owner(),
        mode: source.mode() & 0o7777,
        read_only: true,
        no_copy: true,
    })?;
    let job_facts_directory = directory_from_mount(&job_facts, "/job-facts", "/dev/vda", "ext4");
    binding.job_facts = job_facts;
    binding.observation.job_facts = job_facts_directory;
    binding.channel_digest = binding.computed_digest()?;
    if binding.valid() && binding.matches_initialized_job_facts(ready) {
        Ok(binding)
    } else {
        Err(HostError::Identity)
    }
}

fn directory_from_mount(
    mount: &super::ProofChannelMount,
    mount_root: &str,
    mount_source: &str,
    filesystem_type_name: &str,
) -> ExecutionChannelDirectory {
    ExecutionChannelDirectory {
        target: mount.target().to_owned(),
        inode: mount.directory_inode(),
        owner_uid: mount.owner().0,
        owner_gid: mount.owner().1,
        mode: mount.mode(),
        mount_id: mount.mount_id(),
        statx_mount_id: mount.mount_id(),
        mount_root: mount_root.to_owned(),
        mount_source: mount_source.to_owned(),
        mount_device_major: mount.device().0,
        mount_device_minor: mount.device().1,
        statx_device_major: mount.statx_device().0,
        statx_device_minor: mount.statx_device().1,
        filesystem_type_magic: mount.fs_type_magic(),
        filesystem_type_name: filesystem_type_name.to_owned(),
        filesystem_id: mount.filesystem_id(),
        read_only: mount.read_only(),
        executable_mount: false,
    }
}

pub(crate) fn test_proof_channel_mount(
    fields: TestProofChannelMount,
) -> Result<ProofChannelMount, HostError> {
    let TestProofChannelMount {
        target,
        subpath,
        mount_id,
        device,
        statx_device,
        fs_type_magic,
        filesystem_id,
        filesystem_instance_digest,
        directory_inode,
        owner,
        mode,
        read_only,
        no_copy,
    } = fields;
    let mount = ProofChannelMount {
        target,
        subpath,
        mount_id,
        device_major: device.0,
        device_minor: device.1,
        statx_device_major: statx_device.0,
        statx_device_minor: statx_device.1,
        fs_type_magic,
        filesystem_id,
        filesystem_instance_digest,
        directory_inode,
        owner_uid: owner.0,
        owner_gid: owner.1,
        mode,
        read_only,
        no_copy,
    };
    if !valid_control_mount(&mount) && !valid_inbox_mount(&mount) && !valid_job_facts_mount(&mount)
    {
        return Err(HostError::Journal);
    }
    Ok(mount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_read_only_job_facts_mount_is_valid() {
        let mut binding = test_verified_proof_channel_binding(
            crate::worker::execution_authorization::TestProofChannelBindingFields {
                identity: crate::worker::execution_authorization::TestProofChannelIdentity {
                    reservation_id: 1,
                    operation_id: 1,
                    connection_id: "connection".to_owned(),
                    scale_set_id: 1,
                    witness_request_id: 1,
                    engine_id: "docker-context".to_owned(),
                    docker_root_digest: "a".repeat(64),
                    runner_id: 1,
                    runner_name: "runner".to_owned(),
                    runner_container_id: "runner-container-0001".to_owned(),
                    dind_container_id: "dind-container-00001".to_owned(),
                    instance_binding_digest: "b".repeat(64),
                },
                metadata_volume_name: "wmetadata".to_owned(),
                metadata_volume_id: "metadata-volume-0001".to_owned(),
                control: test_proof_channel_mount(
                    crate::worker::execution_authorization::TestProofChannelMount {
                        target: "/opt/velnor/source-proof/control".to_owned(),
                        subpath: "source-proof/control".to_owned(),
                        mount_id: 21,
                        device: (8, 1),
                        statx_device: (8, 1),
                        fs_type_magic: 0xef53,
                        filesystem_id: 31,
                        filesystem_instance_digest: "c".repeat(64),
                        directory_inode: 101,
                        owner: (0, 0),
                        mode: 0o555,
                        read_only: true,
                        no_copy: true,
                    },
                )
                .expect("The control mount is invalid."),
                inbox: test_proof_channel_mount(
                    crate::worker::execution_authorization::TestProofChannelMount {
                        target: "/opt/velnor/source-proof/inbox".to_owned(),
                        subpath: "source-proof/inbox".to_owned(),
                        mount_id: 22,
                        device: (0, 77),
                        statx_device: (0, 77),
                        fs_type_magic: 0x0102_1994,
                        filesystem_id: 32,
                        filesystem_instance_digest: "d".repeat(64),
                        directory_inode: 102,
                        owner: (1000, 1000),
                        mode: 0o700,
                        read_only: false,
                        no_copy: true,
                    },
                )
                .expect("The inbox mount is invalid."),
                dind_attachment_absent: true,
            },
        )
        .expect("The channel binding is invalid.");

        binding.observation.job_facts.executable_mount = true;
        binding.channel_digest = binding
            .computed_digest()
            .expect("Could not compute the channel digest.");

        assert!(binding.valid());
    }
}
