use super::{
    ProofChannelMount, VerifiedProofChannelBinding, valid_control_mount, valid_inbox_mount,
    valid_job_facts_mount,
};
use crate::error::HostError;
use crate::worker::storage_initialization::VerifiedStorageInitializationReady;
use velnor_resource_probe::execution_channel_wire::{
    ExecutionChannelDirectory, ExecutionChannelObservation,
};

pub(crate) fn test_verified_proof_channel_binding(
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
    inbox: ProofChannelMount,
    dind_attachment_absent: bool,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let control_directory =
        directory_from_mount(&control, "/source-proof/control", "/dev/metadata", "ext4");
    let inbox_directory = directory_from_mount(&inbox, "/source-proof/inbox", "tmpfs", "tmpfs");
    let job_facts = test_proof_channel_mount(
        "/opt/velnor/job-facts".to_owned(),
        "job-facts".to_owned(),
        23,
        (8, 1),
        (8, 1),
        0xef53,
        33,
        "d".repeat(64),
        103,
        (0, 0),
        0o555,
        true,
        true,
    )?;
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

/// Bind a test channel to the exact JobFacts directory in a ready receipt.
pub(crate) fn test_channel_for_ready_job_facts(
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
    ready: &VerifiedStorageInitializationReady,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let mut binding = test_verified_proof_channel_binding(
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
        ready.metadata_volume_name().to_owned(),
        ready.metadata_volume_id().to_owned(),
        test_proof_channel_mount(
            "/opt/velnor/source-proof/control".to_owned(),
            "source-proof/control".to_owned(),
            21,
            (8, 1),
            (8, 1),
            0xef53,
            31,
            "c".repeat(64),
            101,
            (0, 0),
            0o555,
            true,
            true,
        )?,
        test_proof_channel_mount(
            "/opt/velnor/source-proof/inbox".to_owned(),
            "source-proof/inbox".to_owned(),
            22,
            (0, 77),
            (0, 77),
            0x0102_1994,
            32,
            "d".repeat(64),
            102,
            (1000, 1000),
            0o700,
            false,
            true,
        )?,
        true,
    )?;

    let source = ready.job_facts();
    let device = source.device();
    let job_facts = test_proof_channel_mount(
        "/opt/velnor/job-facts".to_owned(),
        "job-facts".to_owned(),
        23,
        device,
        device,
        0xef53,
        33,
        "e".repeat(64),
        source.inode(),
        source.owner(),
        source.mode() & 0o7777,
        true,
        true,
    )?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_read_only_job_facts_mount_is_valid() {
        let mut binding = test_verified_proof_channel_binding(
            1,
            1,
            "connection".to_owned(),
            1,
            1,
            "docker-context".to_owned(),
            "a".repeat(64),
            1,
            "runner".to_owned(),
            "runner-container-0001".to_owned(),
            "dind-container-00001".to_owned(),
            "b".repeat(64),
            "wmetadata".to_owned(),
            "metadata-volume-0001".to_owned(),
            test_proof_channel_mount(
                "/opt/velnor/source-proof/control".to_owned(),
                "source-proof/control".to_owned(),
                21,
                (8, 1),
                (8, 1),
                0xef53,
                31,
                "c".repeat(64),
                101,
                (0, 0),
                0o555,
                true,
                true,
            )
            .expect("valid control mount"),
            test_proof_channel_mount(
                "/opt/velnor/source-proof/inbox".to_owned(),
                "source-proof/inbox".to_owned(),
                22,
                (0, 77),
                (0, 77),
                0x0102_1994,
                32,
                "d".repeat(64),
                102,
                (1000, 1000),
                0o700,
                false,
                true,
            )
            .expect("valid inbox mount"),
            true,
        )
        .expect("valid channel binding");

        binding.observation.job_facts.executable_mount = true;
        binding.channel_digest = binding.computed_digest().expect("channel digest");

        assert!(binding.valid());
    }
}

pub(crate) fn test_proof_channel_mount(
    target: String,
    subpath: String,
    mount_id: u64,
    device: (u32, u32),
    statx_device: (u32, u32),
    fs_type_magic: u64,
    filesystem_id: u64,
    filesystem_instance_digest: String,
    directory_inode: u64,
    owner: (u32, u32),
    mode: u32,
    read_only: bool,
    no_copy: bool,
) -> Result<ProofChannelMount, HostError> {
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
