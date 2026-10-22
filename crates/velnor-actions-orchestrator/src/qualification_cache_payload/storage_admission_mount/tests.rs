use super::{
    GUEST_IMAGE_FILE, MountObservation, read_guest_image_file, validate_mountinfo,
    validate_read_only_mountinfo, validate_work_mountinfo, verify_guest_image_digest,
};
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

#[cfg(target_os = "linux")]
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};

    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

#[test]
fn runtime_identity_requires_one_exact_read_only_mount() {
    let target = "/.velnor-runtime-identity";
    assert_eq!(
        validate_read_only_mountinfo(&line(91, "ro,nosuid,nodev", target), target, 91, 0, 55,),
        Ok(())
    );
    assert_eq!(
        validate_read_only_mountinfo(&line(91, "rw,nosuid,nodev", target), target, 91, 0, 55,),
        Err("cache_storage_mount_not_read_only_bind")
    );
    assert_eq!(
        validate_read_only_mountinfo(
            &line(91, "ro,nosuid,nodev", "/.velnor-runtime-identity/child"),
            target,
            91,
            0,
            55,
        ),
        Err("cache_storage_mount_missing")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn guest_image_digest_checks_bytes_read_through_the_held_directory() {
    use std::{fs, os::unix::fs::PermissionsExt};

    use rustix::fs::{Mode, OFlags, open};

    const CURRENT: &[u8] = br#"{"schema":"velnor.guest-image/v1","source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","guest_platform":"linux/amd64","scale_set":{"name":"runner","labels":["velnor"]},"images":{"runner":{"archive_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","archive_index_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","manifest_digest":"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","config_digest":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"},"dind":{"archive_sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","archive_index_digest":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","manifest_digest":"sha256:1111111111111111111111111111111111111111111111111111111111111111","config_digest":"sha256:2222222222222222222222222222222222222222222222222222222222222222"}}}"#;
    const SWAPPED: &[u8] = br#"{"schema":"velnor.guest-image/v1","source_sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","guest_platform":"linux/amd64","scale_set":{"name":"runner","labels":["velnor"]},"images":{"runner":{},"dind":{}}}"#;

    let root = tempfile::tempdir().expect("temporary root");
    let mounted = root.path().join("runtime-identity");
    let moved = root.path().join("held-runtime-identity");
    fs::create_dir(&mounted).expect("runtime directory");
    let image_path = mounted.join(GUEST_IMAGE_FILE);
    fs::write(&image_path, CURRENT).expect("current image document");
    fs::set_permissions(&image_path, fs::Permissions::from_mode(0o444))
        .expect("read-only image file");
    fs::set_permissions(&mounted, fs::Permissions::from_mode(0o555))
        .expect("read-only runtime directory");
    let directory = open(
        &mounted,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .expect("hold runtime directory descriptor");

    fs::rename(&mounted, &moved).expect("move held directory path");
    fs::create_dir(&mounted).expect("install swapped directory at original path");
    let swapped_path = mounted.join(GUEST_IMAGE_FILE);
    fs::write(&swapped_path, SWAPPED).expect("swapped image document");
    fs::set_permissions(&swapped_path, fs::Permissions::from_mode(0o444))
        .expect("read-only swapped image file");
    fs::set_permissions(&mounted, fs::Permissions::from_mode(0o555))
        .expect("read-only swapped directory");

    let bytes = read_guest_image_file(&directory).expect("read through held directory");
    assert_eq!(bytes, CURRENT);
    let expected = sha256_hex(CURRENT);
    assert_eq!(verify_guest_image_digest(&bytes, &expected), Ok(()));
    let stale = sha256_hex(SWAPPED);
    assert_eq!(
        verify_guest_image_digest(&bytes, &stale),
        Err("cache_storage_guest_image_identity_mismatch")
    );
}
