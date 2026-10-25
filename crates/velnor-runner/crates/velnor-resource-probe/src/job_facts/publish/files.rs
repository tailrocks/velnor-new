use super::{
    FILE_MODE, MOUNTINFO_LIMIT, MOUNTINFO_PATH, ROOT_GID, ROOT_TARGET, ROOT_UID, lower_hex,
    observe_directory,
};
use crate::{
    ProbeError,
    execution_channel_wire::ExecutionChannelDirectory,
    job_facts_wire::JobFactsPublicationFile,
    mountinfo::{MountEntry, attachment_mount, attachment_mount_source},
};
use rustix::fs::{
    AtFlags, Gid, Mode, OFlags, RenameFlags, StatxFlags, Uid, fchmod, fchown, fstat, openat,
    renameat_with, statx,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};

pub(super) fn stage_file(
    directory: &File,
    operation_id: &str,
    name: &str,
    bytes: &[u8],
) -> Result<String, ProbeError> {
    let temporary = format!(".{name}.{operation_id}.tmp");
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
    Ok(temporary)
}

pub(super) fn publish_staged(
    directory: &File,
    temporary: &str,
    name: &str,
) -> Result<(), ProbeError> {
    renameat_with(
        directory,
        temporary,
        directory,
        name,
        RenameFlags::NOREPLACE,
    )
    .map_err(|_| ProbeError::Invalid("job_facts_publish"))?;
    directory
        .sync_all()
        .map_err(|_| ProbeError::Invalid("job_facts_directory_sync"))?;
    Ok(())
}

pub(super) fn inspect_file(
    directory: &File,
    name: &str,
    path: &str,
    expected: &[u8],
    mount: &MountEntry,
    limit: usize,
) -> Result<JobFactsPublicationFile, ProbeError> {
    let descriptor = openat(
        directory,
        name,
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
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("job_facts_readback"))?;
    if bytes != expected
        || bytes.len() > limit
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
        path: path.to_owned(),
        inode: metadata.ino(),
        bytes: metadata.len(),
        owner_uid: metadata.uid(),
        owner_gid: metadata.gid(),
        mode: metadata.mode() & 0o7777,
        readback_sha256: lower_hex(&Sha256::digest(&bytes)),
    })
}

#[derive(Clone, Copy)]
pub(super) struct StableFileState<'a> {
    pub(super) directory: &'a File,
    pub(super) mountinfo: &'a [u8],
    pub(super) mount: &'a MountEntry,
    pub(super) observed: &'a ExecutionChannelDirectory,
    pub(super) file: &'a JobFactsPublicationFile,
    pub(super) name: &'a str,
    pub(super) target: &'a str,
    pub(super) limit: usize,
}

pub(super) fn verify_stable_state(state: &StableFileState<'_>) -> Result<(), ProbeError> {
    let updated_mountinfo = read_mountinfo()?;
    let updated_mount = attachment_mount(&updated_mountinfo, ROOT_TARGET)?;
    let updated_source = attachment_mount_source(&updated_mountinfo, ROOT_TARGET)?;
    let updated_directory = observe_directory(
        state.directory,
        &updated_mount,
        &updated_source,
        state.target,
    )?;
    if updated_mountinfo != state.mountinfo
        || updated_mount != *state.mount
        || updated_directory != *state.observed
    {
        return Err(ProbeError::Invalid("job_facts_state_changed"));
    }
    let measured = inspect_file(
        state.directory,
        state.name,
        &state.file.path,
        &decode_existing_payload(state.directory, state.name, state.file, state.limit)?,
        state.mount,
        state.limit,
    )?;
    if measured != *state.file {
        return Err(ProbeError::Invalid("job_facts_file_changed"));
    }
    Ok(())
}

fn decode_existing_payload(
    directory: &File,
    name: &str,
    expected: &JobFactsPublicationFile,
    limit: usize,
) -> Result<Vec<u8>, ProbeError> {
    let descriptor = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProbeError::Invalid("job_facts_file_open"))?;
    let file = File::from(descriptor);
    let mut bytes = Vec::with_capacity(usize::try_from(expected.bytes).unwrap_or(limit));
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("job_facts_readback"))?;
    if bytes.len() as u64 != expected.bytes
        || lower_hex(&Sha256::digest(&bytes)) != expected.readback_sha256
    {
        return Err(ProbeError::Invalid("job_facts_file_changed"));
    }
    Ok(bytes)
}

pub(super) fn read_mountinfo() -> Result<Vec<u8>, ProbeError> {
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
