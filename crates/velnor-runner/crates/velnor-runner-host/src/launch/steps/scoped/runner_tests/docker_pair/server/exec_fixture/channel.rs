use velnor_resource_probe::execution_channel_wire::{
    ExecutionChannelContext, ExecutionChannelDirectory, ExecutionChannelObservation,
};

pub(crate) fn execution_channel_record(
    context: &ExecutionChannelContext,
    metadata_inodes: [u64; 3],
) -> Result<Vec<u8>, String> {
    let observation = ExecutionChannelObservation {
        schema_version: 3,
        operation_id: context.operation_id.clone(),
        guest_boot_id: context.expected_guest_boot_id.clone(),
        mount_namespace_id: 7001,
        control: channel_directory(
            "/opt/velnor/source-proof/control",
            metadata_inodes[2],
            508,
            "/fixture/source-proof/control",
            "/dev/metadata",
            0,
            48,
            0x9123_683e,
            "btrfs",
            14,
            true,
        ),
        inbox: channel_directory(
            "/opt/velnor/source-proof/inbox",
            609,
            509,
            "/fixture/source-proof/inbox",
            "tmpfs",
            0,
            51,
            0x0102_1994,
            "tmpfs",
            11,
            false,
        ),
        job_facts: channel_directory(
            "/opt/velnor/job-facts",
            metadata_inodes[1],
            506,
            "/fixture/job-facts",
            "/dev/metadata",
            0,
            48,
            0x9123_683e,
            "btrfs",
            14,
            true,
        ),
    };
    let mut bytes = serde_json::to_vec(&observation).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn channel_directory(
    target: &str,
    inode: u64,
    mount_id: u64,
    mount_root: &str,
    mount_source: &str,
    device_major: u32,
    device_minor: u32,
    filesystem_type_magic: u64,
    filesystem_type_name: &str,
    filesystem_id: u64,
    read_only: bool,
) -> ExecutionChannelDirectory {
    let (owner_uid, mode) = if read_only { (0, 0o555) } else { (1000, 0o700) };
    ExecutionChannelDirectory {
        target: target.to_owned(),
        inode,
        owner_uid,
        owner_gid: owner_uid,
        mode,
        mount_id,
        statx_mount_id: mount_id,
        mount_root: mount_root.to_owned(),
        mount_source: mount_source.to_owned(),
        mount_device_major: device_major,
        mount_device_minor: device_minor,
        statx_device_major: device_major,
        statx_device_minor: device_minor,
        filesystem_type_magic,
        filesystem_type_name: filesystem_type_name.to_owned(),
        filesystem_id,
        read_only,
        executable_mount: false,
    }
}
