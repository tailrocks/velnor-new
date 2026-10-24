use super::{
    DirectoryRecord, MountIdentity, ResultRecord, validate_child_mount, validate_directory_state,
    validate_distinct_roots, validate_initial_root, validate_mount_identity,
};
use rustix::{
    fs::{
        AtFlags, FileType, Gid, Mode, OFlags, StatxFlags, Uid, fchmod, fchown, fstat, mkdirat,
        open, openat, statx,
    },
    process::geteuid,
};
use std::{
    fs,
    os::fd::{AsFd, OwnedFd},
};
use velnor_resource_probe::storage_init_contract::METADATA_ROOT_MODE;

const HOME: &str = "/velnor/init/home";
const WORK: &str = "/velnor/init/work";
const METADATA: &str = "/velnor/init/metadata";
const HOME_SOURCE_PROOF: &str = "/velnor/init/home/source-proof";
const HOME_INBOX: &str = "/velnor/init/home/source-proof/inbox";
const RUNTIME_IDENTITY: &str = "/velnor/init/metadata/runtime-identity";
const JOB_FACTS: &str = "/velnor/init/metadata/job-facts";
const SOURCE_PROOF: &str = "/velnor/init/metadata/source-proof";
const CONTROL: &str = "/velnor/init/metadata/source-proof/control";

struct OpenDirectory {
    descriptor: OwnedFd,
    target: &'static str,
    identity: MountIdentity,
}

pub(super) fn initialize(operation_id: String) -> Result<ResultRecord, &'static str> {
    if geteuid().as_raw() != 0 {
        return Err("root_required");
    }
    let home = open_directory(HOME)?;
    let work = open_directory(WORK)?;
    let metadata = open_directory(METADATA)?;
    validate_distinct_roots(home.identity, work.identity)?;
    validate_distinct_roots(home.identity, metadata.identity)?;
    validate_distinct_roots(work.identity, metadata.identity)?;
    validate_empty_root(&home)?;
    validate_empty_root(&work)?;
    validate_empty_root(&metadata)?;
    set_runner_owner(&home)?;
    set_runner_owner(&work)?;
    let home_source_proof = create_child_directory(&home, "source-proof", HOME_SOURCE_PROOF)?;
    let inbox = create_child_directory(&home_source_proof, "inbox", HOME_INBOX)?;
    let runtime_identity = create_child_directory(&metadata, "runtime-identity", RUNTIME_IDENTITY)?;
    let job_facts = create_child_directory(&metadata, "job-facts", JOB_FACTS)?;
    let source_proof = create_child_directory(&metadata, "source-proof", SOURCE_PROOF)?;
    let control = create_child_directory(&source_proof, "control", CONTROL)?;
    let metadata_record = finalize_metadata_root(&metadata, 0, 0)?;
    set_directory_owner(&home_source_proof, 0, 0, 0o755)?;
    set_directory_owner(&inbox, 1000, 1000, 0o700)?;
    set_directory_owner(&runtime_identity, 0, 0, 0o555)?;
    set_directory_owner(&job_facts, 0, 0, 0o555)?;
    set_directory_owner(&source_proof, 0, 0, 0o755)?;
    set_directory_owner(&control, 0, 0, 0o555)?;
    Ok(ResultRecord {
        schema: 2,
        operation_id,
        home: verify_result(&home, 1000, 1000, 0o755)?,
        work: verify_result(&work, 1000, 1000, 0o755)?,
        metadata: metadata_record,
        runtime_identity: verify_result(&runtime_identity, 0, 0, 0o555)?,
        job_facts: verify_result(&job_facts, 0, 0, 0o555)?,
        source_proof: verify_result(&source_proof, 0, 0, 0o755)?,
        control: verify_result(&control, 0, 0, 0o555)?,
        inbox: verify_result(&inbox, 1000, 1000, 0o700)?,
    })
}

fn create_child_directory(
    parent: &OpenDirectory,
    name: &'static str,
    target: &'static str,
) -> Result<OpenDirectory, &'static str> {
    mkdirat(&parent.descriptor, name, Mode::from_raw_mode(0o700)).map_err(|_| "child_create")?;
    let descriptor = openat(
        &parent.descriptor,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "child_open")?;
    let parent_stat = statx(
        &parent.descriptor,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| "child_parent_mount")?;
    let child_stat = statx(
        &descriptor,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| "child_mount")?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if parent_stat.stx_mask & required.bits() != required.bits()
        || child_stat.stx_mask & required.bits() != required.bits()
    {
        return Err("child_mount_fields");
    }
    validate_child_mount(parent_stat.stx_mnt_id, child_stat.stx_mnt_id)?;
    if FileType::from_raw_mode(fstat(&descriptor).map_err(|_| "child_stat")?.st_mode)
        != FileType::Directory
    {
        return Err("child_type");
    }
    Ok(OpenDirectory {
        descriptor,
        target,
        identity: MountIdentity {
            mount_id: child_stat.stx_mnt_id,
            parent_mount_id: parent_stat.stx_mnt_id,
            device_major: child_stat.stx_dev_major,
            device_minor: child_stat.stx_dev_minor,
            inode: child_stat.stx_ino,
        },
    })
}

fn set_directory_owner(
    directory: &OpenDirectory,
    uid: u32,
    gid: u32,
    mode: u32,
) -> Result<(), &'static str> {
    fchown(
        &directory.descriptor,
        Some(Uid::from_raw(uid)),
        Some(Gid::from_raw(gid)),
    )
    .map_err(|_| "target_chown")?;
    fchmod(&directory.descriptor, Mode::from_raw_mode(mode)).map_err(|_| "target_chmod")
}

fn finalize_metadata_root(
    directory: &OpenDirectory,
    uid: u32,
    gid: u32,
) -> Result<DirectoryRecord, &'static str> {
    set_directory_owner(directory, uid, gid, METADATA_ROOT_MODE)?;
    verify_result(directory, uid, gid, METADATA_ROOT_MODE)
}

fn open_directory(path: &'static str) -> Result<OpenDirectory, &'static str> {
    let (parent_path, name) = path.rsplit_once('/').ok_or("target_path")?;
    let parent = open_directory_path(parent_path)?;
    let descriptor = openat(
        &parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "target_open")?;
    let parent_identity = statx(
        &parent,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| "parent_mount")?;
    let identity = statx(
        &descriptor,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| "target_mount")?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if parent_identity.stx_mask & required.bits() != required.bits()
        || identity.stx_mask & required.bits() != required.bits()
    {
        return Err("target_mount_fields");
    }
    let mount_identity = MountIdentity {
        mount_id: identity.stx_mnt_id,
        parent_mount_id: parent_identity.stx_mnt_id,
        device_major: identity.stx_dev_major,
        device_minor: identity.stx_dev_minor,
        inode: identity.stx_ino,
    };
    validate_mount_identity(mount_identity)?;
    if FileType::from_raw_mode(fstat(&descriptor).map_err(|_| "target_stat")?.st_mode)
        != FileType::Directory
    {
        return Err("target_type");
    }
    Ok(OpenDirectory {
        descriptor,
        target: path,
        identity: mount_identity,
    })
}

fn open_directory_path(path: &str) -> Result<OwnedFd, &'static str> {
    let root = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "root_open")?;
    let mut descriptor = root;
    for component in path.split('/').skip(1) {
        descriptor = openat(
            &descriptor,
            component,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| "parent_open")?;
    }
    Ok(descriptor)
}

fn validate_empty_root(directory: &OpenDirectory) -> Result<(), &'static str> {
    let status = fstat(&directory.descriptor).map_err(|_| "target_stat")?;
    let entries = fs::read_dir(format!(
        "/proc/self/fd/{}",
        directory.descriptor.as_fd().as_raw_fd()
    ))
    .map_err(|_| "target_read")?;
    validate_initial_root(
        status.st_uid,
        status.st_gid,
        entries.into_iter().next().is_none(),
    )
}

fn set_runner_owner(directory: &OpenDirectory) -> Result<(), &'static str> {
    fchown(
        &directory.descriptor,
        Some(Uid::from_raw(1000)),
        Some(Gid::from_raw(1000)),
    )
    .map_err(|_| "target_chown")?;
    fchmod(&directory.descriptor, Mode::from_raw_mode(0o755)).map_err(|_| "target_chmod")
}

fn verify_result(
    directory: &OpenDirectory,
    required_user: u32,
    required_group: u32,
    expected_mode: u32,
) -> Result<DirectoryRecord, &'static str> {
    let status = fstat(&directory.descriptor).map_err(|_| "target_verify")?;
    let mode = status.st_mode & 0o7777;
    validate_directory_state(
        status.st_uid,
        status.st_gid,
        mode,
        required_user,
        required_group,
        expected_mode,
    )?;
    Ok(DirectoryRecord {
        target: directory.target,
        mount_id: directory.identity.mount_id,
        parent_mount_id: directory.identity.parent_mount_id,
        device_major: directory.identity.device_major,
        device_minor: directory.identity.device_minor,
        inode: directory.identity.inode,
        uid: status.st_uid,
        gid: status.st_gid,
        mode,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        METADATA, METADATA_ROOT_MODE, MountIdentity, OpenDirectory, finalize_metadata_root,
    };
    use rustix::fs::{AtFlags, Mode, OFlags, StatxFlags, fstat, open, statx};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TemporaryDirectory(PathBuf);

    impl TemporaryDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "velnor-storage-init-{}-{}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create temporary directory");
            Self(path)
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn metadata_root_finalization_serializes_the_observed_read_only_mode() {
        assert_eq!(METADATA_ROOT_MODE, 0o555);

        let temporary = TemporaryDirectory::new();
        let path = temporary.0.to_str().expect("temporary path is UTF-8");
        let descriptor = open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .expect("open temporary directory");
        let parent_path = temporary
            .0
            .parent()
            .expect("temporary directory has a parent")
            .to_str()
            .expect("temporary parent path is UTF-8");
        let parent = open(
            parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .expect("open temporary parent");
        let directory_stat = statx(
            &descriptor,
            "",
            AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
        )
        .expect("stat temporary directory");
        let parent_stat = statx(
            &parent,
            "",
            AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
        )
        .expect("stat temporary parent");
        let status = fstat(&descriptor).expect("read temporary directory owner");
        let directory = OpenDirectory {
            descriptor,
            target: METADATA,
            identity: MountIdentity {
                mount_id: directory_stat.stx_mnt_id,
                parent_mount_id: parent_stat.stx_mnt_id,
                device_major: directory_stat.stx_dev_major,
                device_minor: directory_stat.stx_dev_minor,
                inode: directory_stat.stx_ino,
            },
        };

        let record = finalize_metadata_root(&directory, status.st_uid, status.st_gid)
            .expect("finalize metadata directory");
        assert_eq!(record.mode, METADATA_ROOT_MODE);
        assert_eq!(
            fstat(&directory.descriptor)
                .expect("read finalized mode")
                .st_mode
                & 0o7777,
            0o555
        );

        let encoded = serde_json::to_vec(&record).expect("serialize producer record");
        let decoded: serde_json::Value =
            serde_json::from_slice(&encoded).expect("decode producer record");
        assert_eq!(decoded["mode"], u64::from(METADATA_ROOT_MODE));
    }
}

use std::os::fd::AsRawFd;
