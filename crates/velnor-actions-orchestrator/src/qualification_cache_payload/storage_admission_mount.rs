//! Read the controller-owned storage projection through its read-only mount.

use super::super::storage_admission_types::JobFactsMountV1;

const JOB_FACTS_DIRECTORY: &str = "/opt/velnor/job-facts";
const JOB_FACTS_FILE: &str = "storage-cache-admission.json";
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const MAX_MOUNTINFO_BYTES: usize = 1024 * 1024;
const ROOT_UID: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MountObservation {
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    directory_device: u64,
    directory_inode: u64,
}

impl MountObservation {
    pub(super) fn agrees_with(self, expected: &JobFactsMountV1) -> bool {
        self.mount_id == expected.guest_mount_id
            && self.device_major == expected.guest_device_major
            && self.device_minor == expected.guest_device_minor
            && self.directory_device == expected.guest_directory_device
            && self.directory_inode == expected.guest_directory_inode
            && self.directory_device == expected.host_directory_device
            && self.directory_inode == expected.host_directory_inode
    }
}

/// Read bounded projection bytes and record the actual fixed mount identity.
pub(super) fn read_projection() -> Result<(Vec<u8>, MountObservation), &'static str> {
    #[cfg(target_os = "linux")]
    {
        read_linux_projection()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("cache_storage_mount_unavailable")
    }
}

#[cfg(target_os = "linux")]
fn read_linux_projection() -> Result<(Vec<u8>, MountObservation), &'static str> {
    use rustix::fs::{AtFlags, FileType, Mode, OFlags, StatxFlags, open, openat, statx};
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::AsFd;
    use std::path::{Component, Path};

    let mut directory = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_storage_mount_open_failed")?;
    for component in Path::new(JOB_FACTS_DIRECTORY).components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err("cache_storage_mount_path_invalid");
        };
        let next = openat(
            &directory,
            name,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| "cache_storage_mount_open_failed")?;
        let stat = rustix::fs::fstat(&next).map_err(|_| "cache_storage_mount_stat_failed")?;
        let mode = u32::from(stat.st_mode) & 0o7777;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
            || stat.st_uid != ROOT_UID
            || mode & 0o022 != 0
        {
            return Err("cache_storage_mount_parent_invalid");
        }
        if name == "job-facts" && mode != 0o555 {
            return Err("cache_storage_mount_directory_invalid");
        }
        directory = next;
    }

    let directory_stat =
        rustix::fs::fstat(&directory).map_err(|_| "cache_storage_mount_stat_failed")?;
    let directory_x = statx(
        &directory,
        "",
        AtFlags::EMPTY_PATH,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| "cache_storage_mount_stat_failed")?;
    let mask = StatxFlags::from_bits_retain(directory_x.stx_mask);
    if !mask.contains(StatxFlags::BASIC_STATS | StatxFlags::MNT_ID)
        || FileType::from_raw_mode(directory_stat.st_mode) != FileType::Directory
        || directory_x.stx_uid != ROOT_UID
        || u32::from(directory_stat.st_mode) & 0o7777 != 0o555
        || directory_stat.st_nlink < 2
    {
        return Err("cache_storage_mount_directory_invalid");
    }
    let device =
        u64::try_from(directory_stat.st_dev).map_err(|_| "cache_storage_mount_device_invalid")?;
    if directory_x.stx_ino != directory_stat.st_ino
        || u64::from(directory_x.stx_dev_major) != u64::from(rustix::fs::major(device))
        || u64::from(directory_x.stx_dev_minor) != u64::from(rustix::fs::minor(device))
    {
        return Err("cache_storage_mount_identity_mismatch");
    }
    let observation = MountObservation {
        mount_id: directory_x.stx_mnt_id,
        device_major: directory_x.stx_dev_major,
        device_minor: directory_x.stx_dev_minor,
        directory_device: device,
        directory_inode: directory_stat.st_ino,
    };

    let mut mountinfo =
        File::open(MOUNTINFO_PATH).map_err(|_| "cache_storage_mountinfo_read_failed")?;
    let mut mountinfo_bytes = Vec::new();
    mountinfo
        .take(u64::try_from(MAX_MOUNTINFO_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut mountinfo_bytes)
        .map_err(|_| "cache_storage_mountinfo_read_failed")?;
    if mountinfo_bytes.len() > MAX_MOUNTINFO_BYTES {
        return Err("cache_storage_mountinfo_too_large");
    }
    let mountinfo =
        std::str::from_utf8(&mountinfo_bytes).map_err(|_| "cache_storage_mountinfo_invalid")?;
    super::mountinfo::validate_read_only_mountinfo(
        mountinfo,
        JOB_FACTS_DIRECTORY,
        observation.mount_id,
        observation.device_major,
        observation.device_minor,
    )?;

    let file_descriptor = openat(
        &directory,
        JOB_FACTS_FILE,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_storage_projection_open_failed")?;
    let file_stat =
        rustix::fs::fstat(&file_descriptor).map_err(|_| "cache_storage_projection_stat_failed")?;
    if FileType::from_raw_mode(file_stat.st_mode) != FileType::RegularFile
        || file_stat.st_uid != ROOT_UID
        || u32::from(file_stat.st_mode) & 0o7777 != 0o444
        || file_stat.st_nlink != 1
        || file_stat.st_size <= 0
        || u64::try_from(file_stat.st_size).unwrap_or(u64::MAX) > 64 * 1024
    {
        return Err("cache_storage_projection_file_invalid");
    }
    let mut file = File::from(file_descriptor);
    let mut bytes = Vec::with_capacity(
        usize::try_from(file_stat.st_size)
            .unwrap_or_default()
            .min(64 * 1024),
    );
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cache_storage_projection_read_failed")?;
    let after =
        rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_storage_projection_stat_failed")?;
    if bytes.is_empty()
        || bytes.len() > 64 * 1024
        || file_stat.st_dev != after.st_dev
        || file_stat.st_ino != after.st_ino
        || file_stat.st_mode != after.st_mode
        || file_stat.st_nlink != after.st_nlink
        || file_stat.st_size != after.st_size
        || file_stat.st_mtime != after.st_mtime
    {
        return Err("cache_storage_projection_changed");
    }
    Ok((bytes, observation))
}
