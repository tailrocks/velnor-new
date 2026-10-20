//! Read the controller-owned storage projection through its read-only mount.

use super::super::storage_admission_types::JobFactsMountV1;

#[path = "storage_admission_mount/mountinfo.rs"]
mod mountinfo;
use mountinfo::{read_mountinfo, validate_mountinfo, validate_work_mountinfo};

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
        self.device_major == expected.guest_device_major
            && self.device_minor == expected.guest_device_minor
            && self.directory_device == expected.guest_directory_device
            && self.directory_inode == expected.guest_directory_inode
            && self.directory_device == expected.host_directory_device
            && self.directory_inode == expected.host_directory_inode
    }
}

/// This function reads no more than the byte limit.
/// It returns the mount identity that it observes.
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
    let directory = open_job_facts_directory()?;
    let observation = observe_job_facts_mount(&directory)?;
    validate_current_mountinfo(observation)?;
    let bytes = read_projection_file(&directory)?;
    Ok((bytes, observation))
}

#[cfg(target_os = "linux")]
fn open_job_facts_directory() -> Result<rustix::fd::OwnedFd, &'static str> {
    use rustix::fs::{Mode, OFlags, open, openat};
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
        validate_job_facts_parent(&stat, name == "job-facts")?;
        directory = next;
    }
    Ok(directory)
}

#[cfg(target_os = "linux")]
fn validate_job_facts_parent(
    stat: &rustix::fs::Stat,
    is_job_facts: bool,
) -> Result<(), &'static str> {
    use rustix::fs::FileType;

    let mode = stat.st_mode & 0o7777;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != ROOT_UID
        || mode & 0o022 != 0
    {
        return Err("cache_storage_mount_parent_invalid");
    }
    if is_job_facts && mode != 0o555 {
        return Err("cache_storage_mount_directory_invalid");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn observe_job_facts_mount(
    directory: &rustix::fd::OwnedFd,
) -> Result<MountObservation, &'static str> {
    use rustix::fs::{AtFlags, FileType, StatxFlags, statx};

    let directory_stat =
        rustix::fs::fstat(directory).map_err(|_| "cache_storage_mount_stat_failed")?;
    let directory_x = statx(
        directory,
        "",
        AtFlags::EMPTY_PATH,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| "cache_storage_mount_stat_failed")?;
    let mask = StatxFlags::from_bits_retain(directory_x.stx_mask);
    if !mask.contains(StatxFlags::BASIC_STATS | StatxFlags::MNT_ID)
        || FileType::from_raw_mode(directory_stat.st_mode) != FileType::Directory
        || directory_x.stx_uid != ROOT_UID
        || directory_stat.st_mode & 0o7777 != 0o555
        || directory_stat.st_nlink < 2
    {
        return Err("cache_storage_mount_directory_invalid");
    }
    let device = directory_stat.st_dev;
    if directory_x.stx_ino != directory_stat.st_ino
        || directory_x.stx_dev_major != rustix::fs::major(device)
        || directory_x.stx_dev_minor != rustix::fs::minor(device)
    {
        return Err("cache_storage_mount_identity_mismatch");
    }
    Ok(MountObservation {
        mount_id: directory_x.stx_mnt_id,
        device_major: directory_x.stx_dev_major,
        device_minor: directory_x.stx_dev_minor,
        directory_device: device,
        directory_inode: directory_stat.st_ino,
    })
}

#[cfg(target_os = "linux")]
fn validate_current_mountinfo(observation: MountObservation) -> Result<(), &'static str> {
    let mountinfo = read_mountinfo()?;
    validate_mountinfo(&mountinfo, observation)
}

pub(super) fn validate_current_work_mountinfo(
    target: &std::path::Path,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
) -> Result<(), &'static str> {
    let mountinfo = read_mountinfo()?;
    let target = target
        .to_str()
        .filter(|value| {
            value.starts_with('/')
                && !value
                    .bytes()
                    .any(|byte| byte.is_ascii_whitespace() || byte == b'\\')
        })
        .ok_or("cache_storage_mount_path_invalid")?;
    validate_work_mountinfo(&mountinfo, target, mount_id, device_major, device_minor)
}

#[cfg(target_os = "linux")]
fn read_projection_file(directory: &rustix::fd::OwnedFd) -> Result<Vec<u8>, &'static str> {
    use rustix::fs::{FileType, Mode, OFlags, openat};
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::AsFd;

    let file_descriptor = openat(
        directory,
        JOB_FACTS_FILE,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_storage_projection_open_failed")?;
    let file_stat =
        rustix::fs::fstat(&file_descriptor).map_err(|_| "cache_storage_projection_stat_failed")?;
    if FileType::from_raw_mode(file_stat.st_mode) != FileType::RegularFile
        || file_stat.st_uid != ROOT_UID
        || file_stat.st_mode & 0o7777 != 0o444
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
    (&mut file)
        .take(64 * 1024 + 1)
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
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{MountObservation, validate_mountinfo, validate_work_mountinfo};
    use crate::qualification_cache_payload::storage_admission_types::JobFactsMountV1;

    fn observation() -> MountObservation {
        MountObservation {
            mount_id: 42,
            device_major: 0,
            device_minor: 55,
            directory_device: 55,
            directory_inode: 77,
        }
    }

    fn line(mount_id: u64, options: &str, target: &str) -> String {
        format!("{mount_id} 25 0:55 / {target} {options} rw,relatime - tmpfs tmpfs rw\n")
    }

    #[test]
    fn mountinfo_requires_one_exact_read_only_mount_identity() {
        assert_eq!(
            validate_mountinfo(
                &line(42, "ro,nosuid,nodev", "/opt/velnor/job-facts"),
                observation()
            ),
            Ok(())
        );
        assert_eq!(
            validate_mountinfo(
                &line(41, "ro,nosuid,nodev", "/opt/velnor/job-facts"),
                observation()
            ),
            Err("cache_storage_mount_not_read_only_bind")
        );
        assert_eq!(
            validate_mountinfo(
                &line(42, "rw,nosuid,nodev", "/opt/velnor/job-facts"),
                observation()
            ),
            Err("cache_storage_mount_not_read_only_bind")
        );
    }

    #[test]
    fn job_facts_checks_stable_directory_identity_across_namespaces() {
        let expected = JobFactsMountV1 {
            target: "/opt/velnor/job-facts".to_owned(),
            read_only: true,
            host_directory_device: 55,
            host_directory_inode: 77,
            guest_mount_id: 5,
            guest_device_major: 0,
            guest_device_minor: 55,
            guest_directory_device: 55,
            guest_directory_inode: 77,
        };
        let observed = observation();
        assert_ne!(observed.mount_id, expected.guest_mount_id);
        assert!(observed.agrees_with(&expected));
        let mut foreign = expected;
        foreign.guest_directory_inode += 1;
        assert!(!observed.agrees_with(&foreign));
    }

    #[test]
    fn mountinfo_rejects_duplicate_targets_and_nonexact_paths() {
        let duplicated = format!(
            "{}{}",
            line(42, "ro", "/opt/velnor/job-facts"),
            line(42, "ro", "/opt/velnor/job-facts")
        );
        assert_eq!(
            validate_mountinfo(&duplicated, observation()),
            Err("cache_storage_mount_ambiguous")
        );
        assert_eq!(
            validate_mountinfo(
                &line(42, "ro", "/opt/velnor/job-facts/child"),
                observation()
            ),
            Err("cache_storage_mount_missing")
        );
    }

    #[test]
    fn work_mountinfo_uses_the_job_namespace_identity() {
        assert_eq!(
            validate_work_mountinfo(
                "87 25 0:55 / /__w rw,relatime - ext4 /dev/vda rw\n",
                "/__w",
                87,
                0,
                55,
            ),
            Ok(())
        );
        assert_eq!(
            validate_work_mountinfo(
                "42 25 0:55 / /__w rw,relatime - ext4 /dev/vda rw\n",
                "/__w",
                87,
                0,
                55,
            ),
            Err("cache_storage_mount_not_read_only_bind")
        );
        assert_eq!(
            validate_work_mountinfo(
                "87 25 0:55 / /__w ro,relatime - ext4 /dev/vda ro\n",
                "/__w",
                87,
                0,
                55,
            ),
            Err("cache_storage_mount_not_read_only_bind")
        );
    }

    #[test]
    fn temp_and_work_mounts_use_the_deepest_local_mount() {
        let mountinfo = concat!(
            "87 25 0:55 / /__w rw,relatime - ext4 /dev/vda rw\n",
            "88 87 0:56 / /__w/_temp rw,relatime - ext4 /dev/vdb rw\n",
        );
        assert_eq!(
            validate_work_mountinfo(mountinfo, "/__w/_temp", 88, 0, 56),
            Ok(())
        );
        assert_eq!(
            validate_work_mountinfo(mountinfo, "/__w/_temp/cache", 88, 0, 56),
            Ok(())
        );
        assert_eq!(
            validate_work_mountinfo(mountinfo, "/__w/_temp", 87, 0, 55),
            Err("cache_storage_mount_not_read_only_bind")
        );
        assert_eq!(
            validate_work_mountinfo(mountinfo, "/__w", 87, 0, 55),
            Ok(())
        );
    }
}
