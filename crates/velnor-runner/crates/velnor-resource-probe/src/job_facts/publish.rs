use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};

use super::{JOB_FACTS_RECORD_LIMIT, JOB_FACTS_REQUEST_LIMIT};
use rustix::{
    fs::{
        AtFlags, FileType, Gid, Mode, OFlags, RenameFlags, StatxFlags, Uid, fchmod, fchown, fstat,
        fstatfs, fstatvfs, open, openat, renameat_with, statx,
    },
    process::geteuid,
};
use sha2::{Digest, Sha256};

use crate::{
    ProbeError,
    execution_channel_wire::ExecutionChannelDirectory,
    job_facts_wire::{
        JobFactsPublicationContext, JobFactsPublicationFile, JobFactsPublicationRecord,
    },
    mountinfo::{MountEntry, attachment_mount, attachment_mount_source},
};

const ROOT_TARGET: &str = "/opt/velnor";
const DIRECTORY_TARGET: &str = "/opt/velnor/job-facts";
const FILE_NAME: &str = "storage-cache-admission.json";
const FILE_PATH: &str = "/opt/velnor/job-facts/storage-cache-admission.json";
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const MOUNTINFO_LIMIT: usize = 64 * 1024;
const PAYLOAD_LIMIT: usize = 64 * 1024;
const ROOT_UID: u32 = 0;
const ROOT_GID: u32 = 0;
const DIRECTORY_MODE: u32 = 0o555;
const FILE_MODE: u32 = 0o444;
const BTRFS_MAGIC: u64 = 0x9123_683e;

mod validation;
#[cfg(test)]
use validation::valid_document_shape;
use validation::{decode_payload, lower_hex, validate_context};

#[cfg(test)]
mod tests;

pub(super) fn publish(
    context: JobFactsPublicationContext,
) -> Result<JobFactsPublicationRecord, ProbeError> {
    validate_context(&context)?;
    if serde_json::to_vec(&context)
        .map_err(|_| ProbeError::Invalid("job_facts_request"))?
        .len()
        > JOB_FACTS_REQUEST_LIMIT
    {
        return Err(ProbeError::InputTooLarge("job_facts_request"));
    }
    if geteuid().as_raw() != ROOT_UID {
        return Err(ProbeError::Invalid("job_facts_root_required"));
    }
    let bytes = decode_payload(&context.payload_hex)?;
    let boot_id = crate::execution_channel::identity::read_boot_id()?;
    if boot_id != context.expected_guest_boot_id {
        return Err(ProbeError::Invalid("job_facts_boot_id"));
    }
    let namespace = crate::execution_channel::identity::mount_namespace_id()?;
    let mountinfo = read_mountinfo()?;
    let mount = attachment_mount(&mountinfo, ROOT_TARGET)?;
    if mount.read_only {
        return Err(ProbeError::Invalid("job_facts_mount_read_only"));
    }
    let source = attachment_mount_source(&mountinfo, ROOT_TARGET)?;
    let directory = open_directory()?;
    let observed = observe_directory(&directory, &mount, &source)?;
    compare_runner_directory(&context.expected_runner_job_facts, &observed)?;
    let file = create_and_read(&directory, &context.operation_id, &bytes, &mount)?;
    verify_stable_state(&directory, &mountinfo, &mount, &observed, &file)?;
    let output = JobFactsPublicationRecord {
        schema_version: 1,
        operation_id: context.operation_id,
        guest_boot_id: boot_id,
        mount_namespace_id: namespace,
        payload_sha256: context.payload_sha256,
        directory: observed,
        file,
    };
    if serde_json::to_vec(&output)
        .map_err(|_| ProbeError::Invalid("job_facts_record"))?
        .len()
        > JOB_FACTS_RECORD_LIMIT
    {
        return Err(ProbeError::InputTooLarge("job_facts_record"));
    }
    Ok(output)
}

fn open_directory() -> Result<File, ProbeError> {
    let mut current = File::from(
        open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ProbeError::Invalid("job_facts_root_open"))?,
    );
    for component in ["opt", "velnor", "job-facts"] {
        current = File::from(
            openat(
                &current,
                component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| ProbeError::Invalid("job_facts_directory_open"))?,
        );
    }
    Ok(current)
}

fn observe_directory(
    directory: &File,
    mount: &MountEntry,
    source: &str,
) -> Result<ExecutionChannelDirectory, ProbeError> {
    let stat = fstat(directory).map_err(|_| ProbeError::Invalid("job_facts_directory_stat"))?;
    let statx_value = statx(
        directory,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| ProbeError::Invalid("job_facts_directory_statx"))?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if statx_value.stx_mask & required.bits() != required.bits()
        || FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != ROOT_UID
        || stat.st_gid != ROOT_GID
        || stat.st_mode & 0o7777 != DIRECTORY_MODE
        || stat.st_ino == 0
        || statx_value.stx_ino != stat.st_ino
        || statx_value.stx_uid != ROOT_UID
        || statx_value.stx_gid != ROOT_GID
        || statx_value.stx_mnt_id != mount.mount_id
        || mount.read_only
    {
        return Err(ProbeError::Invalid("job_facts_directory_identity"));
    }
    let filesystem =
        fstatfs(directory).map_err(|_| ProbeError::Invalid("job_facts_directory_statfs"))?;
    let capacity =
        fstatvfs(directory).map_err(|_| ProbeError::Invalid("job_facts_directory_statvfs"))?;
    let magic = u64::try_from(filesystem.f_type)
        .map_err(|_| ProbeError::Invalid("job_facts_filesystem_type"))?;
    Ok(ExecutionChannelDirectory {
        target: DIRECTORY_TARGET.to_owned(),
        inode: stat.st_ino,
        owner_uid: ROOT_UID,
        owner_gid: ROOT_GID,
        mode: stat.st_mode & 0o7777,
        mount_id: mount.mount_id,
        statx_mount_id: statx_value.stx_mnt_id,
        mount_root: mount.root.clone(),
        mount_source: source.to_owned(),
        mount_device_major: mount.device_major,
        mount_device_minor: mount.device_minor,
        statx_device_major: statx_value.stx_dev_major,
        statx_device_minor: statx_value.stx_dev_minor,
        filesystem_type_magic: magic,
        filesystem_type_name: mount.filesystem_type.clone(),
        filesystem_id: capacity.f_fsid,
        read_only: false,
        executable_mount: mount.executable,
    })
}

fn compare_runner_directory(
    expected: &ExecutionChannelDirectory,
    observed: &ExecutionChannelDirectory,
) -> Result<(), ProbeError> {
    let btrfs = observed.filesystem_type_magic == BTRFS_MAGIC;
    if observed.target != expected.target
        || observed.inode != expected.inode
        || observed.owner_uid != expected.owner_uid
        || observed.owner_gid != expected.owner_gid
        || observed.mode != expected.mode
        || observed.filesystem_type_magic != expected.filesystem_type_magic
        || observed.filesystem_type_name != expected.filesystem_type_name
        || observed.filesystem_id != expected.filesystem_id
        || observed.mount_root != expected.mount_root
        || observed.mount_source != expected.mount_source
        || observed.mount_device_major != expected.mount_device_major
        || observed.mount_device_minor != expected.mount_device_minor
        || (!btrfs
            && (observed.statx_device_major != expected.statx_device_major
                || observed.statx_device_minor != expected.statx_device_minor))
        || observed.read_only
    {
        return Err(ProbeError::Invalid("job_facts_runner_binding"));
    }
    Ok(())
}

fn create_and_read(
    directory: &File,
    operation_id: &str,
    bytes: &[u8],
    mount: &MountEntry,
) -> Result<JobFactsPublicationFile, ProbeError> {
    let temporary = format!(".storage-cache-admission.{operation_id}.tmp");
    let descriptor = openat(
        directory,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| ProbeError::Invalid("job_facts_temporary_create"))?;
    let mut file = File::from(descriptor);
    file.write_all(bytes)
        .map_err(|_| ProbeError::Invalid("job_facts_write"))?;
    fchown(
        &file,
        Some(Uid::from_raw(ROOT_UID)),
        Some(Gid::from_raw(ROOT_GID)),
    )
    .map_err(|_| ProbeError::Invalid("job_facts_owner"))?;
    fchmod(&file, Mode::from_raw_mode(FILE_MODE))
        .map_err(|_| ProbeError::Invalid("job_facts_mode"))?;
    file.sync_all()
        .map_err(|_| ProbeError::Invalid("job_facts_file_sync"))?;
    renameat_with(
        directory,
        temporary.as_str(),
        directory,
        FILE_NAME,
        RenameFlags::NOREPLACE,
    )
    .map_err(|_| ProbeError::Invalid("job_facts_publish"))?;
    directory
        .sync_all()
        .map_err(|_| ProbeError::Invalid("job_facts_directory_sync"))?;
    inspect_file(directory, bytes, mount)
}

fn inspect_file(
    directory: &File,
    expected: &[u8],
    mount: &MountEntry,
) -> Result<JobFactsPublicationFile, ProbeError> {
    let descriptor = openat(
        directory,
        FILE_NAME,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProbeError::Invalid("job_facts_file_open"))?;
    let file = File::from(descriptor);
    let metadata = file
        .metadata()
        .map_err(|_| ProbeError::Invalid("job_facts_file_stat"))?;
    let stat = fstat(&file).map_err(|_| ProbeError::Invalid("job_facts_file_fstat"))?;
    let statx_value = statx(
        &file,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| ProbeError::Invalid("job_facts_file_statx"))?;
    let mut bytes = Vec::with_capacity(expected.len());
    file.take((PAYLOAD_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("job_facts_readback"))?;
    if bytes != expected
        || bytes.len() > PAYLOAD_LIMIT
        || !metadata.is_file()
        || metadata.uid() != ROOT_UID
        || metadata.gid() != ROOT_GID
        || metadata.mode() & 0o7777 != FILE_MODE
        || metadata.len() != bytes.len() as u64
        || stat.st_ino != metadata.ino()
        || stat.st_mode & 0o7777 != FILE_MODE
        || statx_value.stx_ino != metadata.ino()
        || statx_value.stx_mnt_id != mount.mount_id
    {
        return Err(ProbeError::Invalid("job_facts_file_identity"));
    }
    Ok(JobFactsPublicationFile {
        path: FILE_PATH.to_owned(),
        inode: metadata.ino(),
        bytes: metadata.len(),
        owner_uid: metadata.uid(),
        owner_gid: metadata.gid(),
        mode: metadata.mode() & 0o7777,
        readback_sha256: lower_hex(&Sha256::digest(&bytes)),
    })
}

fn verify_stable_state(
    directory: &File,
    mountinfo: &[u8],
    mount: &MountEntry,
    observed: &ExecutionChannelDirectory,
    file: &JobFactsPublicationFile,
) -> Result<(), ProbeError> {
    let updated_mountinfo = read_mountinfo()?;
    let updated_mount = attachment_mount(&updated_mountinfo, ROOT_TARGET)?;
    let updated_source = attachment_mount_source(&updated_mountinfo, ROOT_TARGET)?;
    let updated_directory = observe_directory(directory, &updated_mount, &updated_source)?;
    if updated_mountinfo != mountinfo || updated_mount != *mount || updated_directory != *observed {
        return Err(ProbeError::Invalid("job_facts_state_changed"));
    }
    let measured = inspect_file(directory, &decode_existing_payload(directory, file)?, mount)?;
    if measured != *file {
        return Err(ProbeError::Invalid("job_facts_file_changed"));
    }
    Ok(())
}

fn decode_existing_payload(
    directory: &File,
    expected: &JobFactsPublicationFile,
) -> Result<Vec<u8>, ProbeError> {
    let descriptor = openat(
        directory,
        FILE_NAME,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProbeError::Invalid("job_facts_file_open"))?;
    let file = File::from(descriptor);
    let mut bytes = Vec::with_capacity(usize::try_from(expected.bytes).unwrap_or(PAYLOAD_LIMIT));
    file.take((PAYLOAD_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("job_facts_readback"))?;
    if bytes.len() as u64 != expected.bytes
        || lower_hex(&Sha256::digest(&bytes)) != expected.readback_sha256
    {
        return Err(ProbeError::Invalid("job_facts_file_changed"));
    }
    Ok(bytes)
}

fn read_mountinfo() -> Result<Vec<u8>, ProbeError> {
    let file =
        File::open(MOUNTINFO_PATH).map_err(|_| ProbeError::Invalid("job_facts_mountinfo_open"))?;
    let mut bytes = Vec::with_capacity(MOUNTINFO_LIMIT.min(4096));
    file.take((MOUNTINFO_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("job_facts_mountinfo_read"))?;
    if bytes.len() > MOUNTINFO_LIMIT {
        return Err(ProbeError::InputTooLarge("job_facts_mountinfo"));
    }
    Ok(bytes)
}
