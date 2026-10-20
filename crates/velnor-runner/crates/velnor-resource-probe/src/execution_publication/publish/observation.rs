use super::{
    AtFlags, CONTROL_DIRECTORY_MODE, CONTROL_FILE_MODE, CONTROL_GID, CONTROL_UID,
    DirectoryIdentity, File, FileIdentity, FileType, MOUNTINFO_LIMIT, MOUNTINFO_PATH, MetadataExt,
    Mode, MountEntry, OFlags, PUBLISHER_ROOT, ProbeError, Read, StatxFlags,
    attachment_mount_source, filesystem_name, fstat, fstatfs, fstatvfs, open, openat, statx,
};

pub(super) fn observe_control_file_identity(
    file: &File,
    directory: &File,
    mount_entry: &MountEntry,
    mountinfo: &[u8],
    metadata: &std::fs::Metadata,
    size: usize,
    owner_uid: u32,
    owner_gid: u32,
) -> Result<FileIdentity, ProbeError> {
    let stat = fstat(file).map_err(|_| ProbeError::Invalid("publication_file_fstat"))?;
    let statx_value = statx(
        file,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| ProbeError::Invalid("publication_file_statx"))?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    let size = size as u64;
    if statx_value.stx_mask & required.bits() != required.bits()
        || FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_ino != metadata.ino()
        || stat.st_uid != owner_uid
        || stat.st_gid != owner_gid
        || stat.st_mode & 0o777 != CONTROL_FILE_MODE
        || stat.st_size < 0
        || u64::try_from(stat.st_size).ok() != Some(size)
        || statx_value.stx_ino != metadata.ino()
        || statx_value.stx_uid != owner_uid
        || statx_value.stx_gid != owner_gid
        || u32::from(statx_value.stx_mode) & 0o777 != CONTROL_FILE_MODE
        || statx_value.stx_size != size
    {
        return Err(ProbeError::Invalid("publication_file_statx_fields"));
    }
    let directory_statx = statx(
        directory,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| ProbeError::Invalid("publication_directory_statx"))?;
    if statx_value.stx_mnt_id != directory_statx.stx_mnt_id {
        return Err(ProbeError::Invalid("publication_file_changed"));
    }
    let fs = fstatfs(file).map_err(|_| ProbeError::Invalid("publication_file_statfs"))?;
    let vfs = fstatvfs(file).map_err(|_| ProbeError::Invalid("publication_file_statvfs"))?;
    let filesystem_type_magic =
        u64::try_from(fs.f_type).map_err(|_| ProbeError::Invalid("publication_file_fs_type"))?;
    if mount_entry.read_only
        || mount_entry.mount_id != statx_value.stx_mnt_id
        || filesystem_type_magic != directory_filesystem_type(directory)?
        || vfs.f_fsid != directory_filesystem_id(directory)?
    {
        return Err(ProbeError::Invalid("publication_file_filesystem"));
    }
    Ok(FileIdentity {
        inode: metadata.ino(),
        owner_uid: metadata.uid(),
        owner_gid: metadata.gid(),
        mode: metadata.mode() & 0o777,
        size,
        mount_id: mount_entry.mount_id,
        statx_mount_id: statx_value.stx_mnt_id,
        mount_device_major: mount_entry.device_major,
        mount_device_minor: mount_entry.device_minor,
        statx_device_major: statx_value.stx_dev_major,
        statx_device_minor: statx_value.stx_dev_minor,
        filesystem_type_magic,
        filesystem_id: vfs.f_fsid,
        mount_root: mount_entry.root.clone(),
        mount_source: attachment_mount_source(mountinfo, PUBLISHER_ROOT)?,
    })
}

pub(super) fn observe_control_directory(
    directory: &File,
    mount: &MountEntry,
    mountinfo: &[u8],
) -> Result<DirectoryIdentity, ProbeError> {
    let metadata = directory
        .metadata()
        .map_err(|_| ProbeError::Invalid("publication_directory_stat"))?;
    let mode = metadata.mode() & 0o777;
    let stat = fstat(directory).map_err(|_| ProbeError::Invalid("publication_directory_fstat"))?;
    let statx_value = statx(
        directory,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| ProbeError::Invalid("publication_directory_statx"))?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if statx_value.stx_mask & required.bits() != required.bits()
        || stat.st_ino != metadata.ino()
        || stat.st_uid != CONTROL_UID
        || stat.st_gid != CONTROL_GID
        || stat.st_mode & 0o777 != CONTROL_DIRECTORY_MODE
        || statx_value.stx_ino != metadata.ino()
        || statx_value.stx_uid != CONTROL_UID
        || statx_value.stx_gid != CONTROL_GID
        || u32::from(statx_value.stx_mode & 0o777) != CONTROL_DIRECTORY_MODE
        || !metadata.is_dir()
        || metadata.uid() != CONTROL_UID
        || metadata.gid() != CONTROL_GID
        || mode != CONTROL_DIRECTORY_MODE
        || mount.mount_id != statx_value.stx_mnt_id
        || mount.read_only
    {
        return Err(ProbeError::Invalid("publication_directory_identity"));
    }
    let fs = fstatfs(directory).map_err(|_| ProbeError::Invalid("publication_directory_statfs"))?;
    let vfs =
        fstatvfs(directory).map_err(|_| ProbeError::Invalid("publication_directory_statvfs"))?;
    let filesystem_type_magic = u64::try_from(fs.f_type)
        .map_err(|_| ProbeError::Invalid("publication_directory_fs_type"))?;
    let filesystem_type_name = filesystem_name(mount, filesystem_type_magic)?;
    Ok(DirectoryIdentity {
        inode: metadata.ino(),
        owner_uid: metadata.uid(),
        owner_gid: metadata.gid(),
        mode,
        mount_id: mount.mount_id,
        statx_mount_id: statx_value.stx_mnt_id,
        mount_device_major: mount.device_major,
        mount_device_minor: mount.device_minor,
        statx_device_major: statx_value.stx_dev_major,
        statx_device_minor: statx_value.stx_dev_minor,
        filesystem_type_magic,
        filesystem_type_name,
        filesystem_id: vfs.f_fsid,
        mount_root: mount.root.clone(),
        mount_source: attachment_mount_source(mountinfo, PUBLISHER_ROOT)?,
        read_only: mount.read_only,
    })
}

pub(super) fn open_fixed_control_directory() -> Result<File, ProbeError> {
    let root = File::from(
        open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ProbeError::Invalid("publication_root_open"))?,
    );
    let mut current = root;
    for component in ["opt", "velnor", "source-proof", "control"] {
        current = File::from(
            openat(
                &current,
                component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| ProbeError::Invalid("publication_directory_open"))?,
        );
    }
    Ok(current)
}

pub(super) fn open_control_directory() -> Result<File, ProbeError> {
    open_fixed_control_directory()
}

pub(super) fn read_mountinfo() -> Result<Vec<u8>, ProbeError> {
    let file = File::open(MOUNTINFO_PATH)
        .map_err(|_| ProbeError::Invalid("publication_mountinfo_open"))?;
    let mut bytes = Vec::with_capacity(MOUNTINFO_LIMIT.min(4096));
    file.take((MOUNTINFO_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("publication_mountinfo_read"))?;
    if bytes.len() > MOUNTINFO_LIMIT {
        return Err(ProbeError::InputTooLarge("publication_mountinfo"));
    }
    Ok(bytes)
}

pub(super) fn decode_payload(value: &str) -> Result<Vec<u8>, ProbeError> {
    if !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProbeError::Invalid("publication_payload_hex"));
    }
    let mut output = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().as_chunks::<2>().0 {
        let high = decode_hex(pair[0]).ok_or(ProbeError::Invalid("publication_payload_hex"))?;
        let low = decode_hex(pair[1]).ok_or(ProbeError::Invalid("publication_payload_hex"))?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

pub(super) fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

pub(super) fn valid_operation_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_boot_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn directory_filesystem_type(directory: &File) -> Result<u64, ProbeError> {
    let fs = fstatfs(directory).map_err(|_| ProbeError::Invalid("publication_directory_statfs"))?;
    u64::try_from(fs.f_type).map_err(|_| ProbeError::Invalid("publication_directory_fs_type"))
}

pub(super) fn directory_filesystem_id(directory: &File) -> Result<u64, ProbeError> {
    fstatvfs(directory)
        .map(|value| value.f_fsid)
        .map_err(|_| ProbeError::Invalid("publication_directory_statvfs"))
}
