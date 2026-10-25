#[cfg(any(target_os = "linux", test))]
use crate::ProbeError;
#[cfg(target_os = "linux")]
use crate::worker_binding::{
    ProcessWitness, RootBackingDirectoryObservation, RootBackingKind, RootBackingObservation,
    WorkerRole,
};

#[path = "root_backing/parser.rs"]
mod parser;
#[cfg(target_os = "linux")]
use parser::backing_mount;
#[cfg(target_os = "linux")]
use parser::{parse_overlay_paths, parse_root_mount};

#[cfg(target_os = "linux")]
const MOUNTINFO_LIMIT: usize = 65_536;
#[cfg(target_os = "linux")]
const ROOT_BIND: &str = "/velnor/docker-root";

pub(crate) use parser::validate_docker_root_path;

#[cfg(target_os = "linux")]
pub(super) fn observe(
    role: WorkerRole,
    container_id: &str,
    guest_boot_id: &str,
    init: &ProcessWitness,
    helper: &ProcessWitness,
    docker_root_path: &str,
    probe_mountinfo: &[u8],
) -> Result<RootBackingObservation, ProbeError> {
    validate_docker_root_path(docker_root_path)?;
    if init.mount_namespace_id != helper.mount_namespace_id {
        return Err(ProbeError::Invalid("root_backing_namespace_mismatch"));
    }

    let init_before = super::process::observe_process(init.host_pid, init.start_ticks)?;
    let helper_before = super::process::observe_process(helper.host_pid, helper.start_ticks)?;
    if &init_before != init || &helper_before != helper {
        return Err(ProbeError::Invalid("root_backing_process_changed"));
    }
    let mountinfo = read_process_mountinfo(init.host_pid)?;
    let mount = parse_root_mount(&mountinfo)?;
    if !mount.read_only || mount.filesystem_type != "overlay" {
        return Err(ProbeError::Invalid("root_backing_overlay_required"));
    }
    let paths = parse_overlay_paths(&mount.super_options)?;
    let mut directories = Vec::with_capacity(paths.lowerdirs.len() + 2);
    directories.push(observe_backing_directory(
        RootBackingKind::Upper,
        &paths.upperdir,
        docker_root_path,
        probe_mountinfo,
    )?);
    directories.push(observe_backing_directory(
        RootBackingKind::Work,
        &paths.workdir,
        docker_root_path,
        probe_mountinfo,
    )?);
    for path in &paths.lowerdirs {
        directories.push(observe_backing_directory(
            RootBackingKind::Lower,
            path,
            docker_root_path,
            probe_mountinfo,
        )?);
    }
    if !same_filesystem(&directories[0], &directories[1]) {
        return Err(ProbeError::Invalid("root_backing_upper_work_mismatch"));
    }

    if read_process_mountinfo(init.host_pid)? != mountinfo
        || super::process::observe_process(init.host_pid, init.start_ticks)? != init_before
        || super::process::observe_process(helper.host_pid, helper.start_ticks)? != helper_before
    {
        return Err(ProbeError::Invalid("root_backing_observation_changed"));
    }

    Ok(RootBackingObservation {
        role,
        container_id: container_id.to_owned(),
        guest_boot_id: guest_boot_id.to_owned(),
        init: init.clone(),
        helper: helper.clone(),
        mount_source: mount.mount_source,
        filesystem_type: mount.filesystem_type,
        mount_id: mount.mount_id,
        mount_device_major: mount.device_major,
        mount_device_minor: mount.device_minor,
        mount_root: mount.mount_root,
        read_only: mount.read_only,
        upperdir: paths.upperdir,
        workdir: paths.workdir,
        lowerdirs: paths.lowerdirs,
        directories,
    })
}

#[cfg(target_os = "linux")]
fn same_filesystem(
    upper: &RootBackingDirectoryObservation,
    work: &RootBackingDirectoryObservation,
) -> bool {
    upper.statx_mount_id == work.statx_mount_id
        && upper.statx_device_major == work.statx_device_major
        && upper.statx_device_minor == work.statx_device_minor
        && upper.filesystem_type_magic == work.filesystem_type_magic
        && upper.filesystem_id == work.filesystem_id
}

#[cfg(target_os = "linux")]
fn read_process_mountinfo(pid: u32) -> Result<Vec<u8>, ProbeError> {
    use std::io::Read;

    let file = std::fs::File::open(format!("/proc/{pid}/mountinfo"))
        .map_err(|error| ProbeError::Read("process_mountinfo", error))?;
    let mut bytes = Vec::with_capacity(4096);
    file.take((MOUNTINFO_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read("process_mountinfo", error))?;
    if bytes.len() > MOUNTINFO_LIMIT {
        return Err(ProbeError::InputTooLarge("process_mountinfo"));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn observe_backing_directory(
    kind: RootBackingKind,
    path: &str,
    docker_root_path: &str,
    probe_mountinfo: &[u8],
) -> Result<RootBackingDirectoryObservation, ProbeError> {
    use rustix::fs::fstatvfs;

    let relative = strip_root_prefix(path, docker_root_path)?;
    let probe_path = format!("{ROOT_BIND}/{relative}");
    let mount = backing_mount(probe_mountinfo, &probe_path)?;
    let descriptor = open_backing_directory(relative)?;
    let (identity, filesystem) = backing_identity(&descriptor)?;
    let filesystem_type_magic = u64::try_from(filesystem.f_type)
        .map_err(|_| ProbeError::Invalid("root_backing_fs_type"))?;
    if mount.mount_id != identity.stx_mnt_id
        || !mount.read_only
        || (filesystem_type_magic == crate::filesystem::BTRFS_SUPER_MAGIC)
            != (mount.filesystem_type == "btrfs")
        || (filesystem_type_magic != crate::filesystem::BTRFS_SUPER_MAGIC
            && (mount.device_major != identity.stx_dev_major
                || mount.device_minor != identity.stx_dev_minor))
    {
        return Err(ProbeError::Invalid("root_backing_mount_identity"));
    }
    let capacity = fstatvfs(&descriptor)
        .map_err(|error| ProbeError::Read("root_backing_statvfs", error.into()))?;
    let block_size = capacity.f_frsize;
    if block_size == 0 {
        return Err(ProbeError::Invalid("root_backing_block_size"));
    }
    let total_bytes = checked_bytes(capacity.f_blocks, block_size)?;
    let free_bytes = checked_bytes(capacity.f_bfree, block_size)?;
    let available_bytes = checked_bytes(capacity.f_bavail, block_size)?;
    if free_bytes > total_bytes || available_bytes > free_bytes {
        return Err(ProbeError::Invalid("root_backing_capacity"));
    }
    if capacity.f_ffree > capacity.f_files || capacity.f_favail > capacity.f_ffree {
        return Err(ProbeError::Invalid("root_backing_inode_capacity"));
    }
    Ok(RootBackingDirectoryObservation {
        kind,
        path: path.to_owned(),
        mount_root: mount.root,
        mount_source: mount.source,
        mount_filesystem_type: mount.filesystem_type,
        mount_read_only: mount.read_only,
        mount_device_major: mount.device_major,
        mount_device_minor: mount.device_minor,
        statx_mount_id: identity.stx_mnt_id,
        statx_device_major: identity.stx_dev_major,
        statx_device_minor: identity.stx_dev_minor,
        filesystem_type_magic,
        filesystem_id: capacity.f_fsid,
        block_size,
        total_bytes,
        free_bytes,
        available_bytes,
        inode_total: capacity.f_files,
        inode_free: capacity.f_ffree,
        inode_available: capacity.f_favail,
    })
}

#[cfg(target_os = "linux")]
fn open_backing_directory(relative: &str) -> Result<std::os::fd::OwnedFd, ProbeError> {
    use rustix::fs::{Mode, OFlags, open, openat};

    let mut descriptor = open(
        ROOT_BIND,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| ProbeError::Read("root_backing_root", error.into()))?;
    for component in relative.split('/') {
        descriptor = openat(
            &descriptor,
            component,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| ProbeError::Read("root_backing_open", error.into()))?;
    }
    Ok(descriptor)
}

#[cfg(target_os = "linux")]
fn backing_identity(
    descriptor: &std::os::fd::OwnedFd,
) -> Result<(rustix::fs::Statx, rustix::fs::StatFs), ProbeError> {
    use rustix::fs::{AtFlags, FileType, StatxFlags, fstat, fstatfs, statx};

    let metadata =
        fstat(descriptor).map_err(|error| ProbeError::Read("root_backing_stat", error.into()))?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::Directory {
        return Err(ProbeError::Invalid("root_backing_not_directory"));
    }
    let identity = statx(
        descriptor,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|error| ProbeError::Read("root_backing_statx", error.into()))?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if identity.stx_mask & required.bits() != required.bits() {
        return Err(ProbeError::Invalid("root_backing_statx_fields"));
    }
    let filesystem = fstatfs(descriptor)
        .map_err(|error| ProbeError::Read("root_backing_statfs", error.into()))?;
    Ok((identity, filesystem))
}

#[cfg(any(target_os = "linux", test))]
fn strip_root_prefix<'a>(path: &'a str, root: &str) -> Result<&'a str, ProbeError> {
    let relative = path
        .strip_prefix(root)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .ok_or(ProbeError::Invalid("root_backing_outside_docker_root"))?;
    if relative.is_empty()
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(ProbeError::Invalid("root_backing_path"));
    }
    Ok(relative)
}

#[cfg(target_os = "linux")]
fn checked_bytes(blocks: u64, block_size: u64) -> Result<u64, ProbeError> {
    blocks
        .checked_mul(block_size)
        .ok_or(ProbeError::Overflow("root_backing_capacity"))
}

#[cfg(test)]
#[path = "root_backing/tests.rs"]
mod tests;
