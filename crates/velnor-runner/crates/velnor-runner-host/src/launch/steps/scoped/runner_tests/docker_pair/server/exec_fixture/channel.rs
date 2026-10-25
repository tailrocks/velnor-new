use velnor_resource_probe::execution_channel_wire::{
    ExecutionChannelContext, ExecutionChannelDirectory, ExecutionChannelObservation,
};

pub(crate) fn execution_channel_record(
    context: &ExecutionChannelContext,
    metadata_inodes: [u64; 3],
    tamper_response: u8,
) -> Result<Vec<u8>, String> {
    let mut observation = ExecutionChannelObservation {
        schema_version: 3,
        operation_id: context.operation_id.clone(),
        guest_boot_id: context.expected_guest_boot_id.clone(),
        mount_namespace_id: 7001,
        control: channel_directory(ChannelDirectorySpec {
            target: "/opt/velnor/source-proof/control",
            inode: metadata_inodes[2],
            mount_id: 508,
            mount_root: "/fixture/source-proof/control",
            mount_source: "/dev/metadata",
            device_major: 0,
            device_minor: 48,
            filesystem_type_magic: 0x9123_683e,
            filesystem_type_name: "btrfs",
            filesystem_id: 14,
            read_only: true,
        }),
        inbox: channel_directory(ChannelDirectorySpec {
            target: "/opt/velnor/source-proof/inbox",
            inode: 609,
            mount_id: 509,
            mount_root: "/fixture/source-proof/inbox",
            mount_source: "tmpfs",
            device_major: 0,
            device_minor: 51,
            filesystem_type_magic: 0x0102_1994,
            filesystem_type_name: "tmpfs",
            filesystem_id: 11,
            read_only: false,
        }),
        job_facts: channel_directory(ChannelDirectorySpec {
            target: "/opt/velnor/job-facts",
            inode: metadata_inodes[1],
            mount_id: 506,
            mount_root: "/fixture/job-facts",
            mount_source: "/dev/metadata",
            device_major: 0,
            device_minor: 48,
            filesystem_type_magic: 0x9123_683e,
            filesystem_type_name: "btrfs",
            filesystem_id: 14,
            read_only: true,
        }),
    };
    match tamper_response {
        1 => observation.operation_id.push('0'),
        2 => observation.job_facts.inode += 1,
        _ => {}
    }
    let mut bytes = serde_json::to_vec(&observation).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Clone, Copy)]
struct ChannelDirectorySpec<'a> {
    target: &'a str,
    inode: u64,
    mount_id: u64,
    mount_root: &'a str,
    mount_source: &'a str,
    device_major: u32,
    device_minor: u32,
    filesystem_type_magic: u64,
    filesystem_type_name: &'a str,
    filesystem_id: u64,
    read_only: bool,
}

fn channel_directory(spec: ChannelDirectorySpec<'_>) -> ExecutionChannelDirectory {
    let ChannelDirectorySpec {
        target,
        inode,
        mount_id,
        mount_root,
        mount_source,
        device_major,
        device_minor,
        filesystem_type_magic,
        filesystem_type_name,
        filesystem_id,
        read_only,
    } = spec;
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
