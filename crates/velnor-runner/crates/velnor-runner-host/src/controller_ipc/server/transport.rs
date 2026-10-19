use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::error::HostError;
use crate::ipc::{MAX_FRAME, encode_frame};

use super::super::wire::{MAX_REQUEST_BYTES, Request, Response};

pub(super) fn read_request(stream: &mut UnixStream) -> Result<Request, HostError> {
    let length = read_frame_length(stream)?;
    if length == 0 || length > MAX_REQUEST_BYTES {
        return Err(HostError::Frame);
    }
    let mut bytes = vec![0; length];
    stream
        .read_exact(&mut bytes)
        .map_err(|_| HostError::Frame)?;
    serde_json::from_slice(&bytes).map_err(|_| HostError::Frame)
}

pub(super) fn write_response(stream: &mut UnixStream, response: Response) -> Result<(), HostError> {
    let payload = response.encode().map_err(|()| HostError::Frame)?;
    stream
        .write_all(&encode_frame(&payload)?)
        .map_err(|_| HostError::Frame)
}

fn read_frame_length(stream: &mut UnixStream) -> Result<usize, HostError> {
    let mut prefix = [0; 4];
    stream
        .read_exact(&mut prefix)
        .map_err(|_| HostError::Frame)?;
    let length = usize::try_from(u32::from_be_bytes(prefix)).map_err(|_| HostError::Frame)?;
    if length > MAX_FRAME {
        return Err(HostError::Frame);
    }
    Ok(length)
}

pub(super) fn trusted_peer(_stream: &UnixStream) -> bool {
    // The private parent directory and 0600 socket restrict connect access to
    // the state owner on macOS; Linux additionally validates SO_PEERCRED.
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsFd;

        let Ok(credentials) = rustix::net::sockopt::socket_peercred(_stream.as_fd()) else {
            return false;
        };
        return credentials.uid == rustix::process::geteuid();
    }
    #[cfg(not(target_os = "linux"))]
    true
}

pub(super) fn private_state_directory(path: &Path) -> Result<PathBuf, HostError> {
    if !path.is_absolute() {
        return Err(HostError::Path);
    }
    fs::create_dir_all(path).map_err(|_| HostError::Path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(HostError::Path);
    }
    let owner = rustix::process::geteuid().as_raw();
    if metadata.uid() != owner {
        return Err(HostError::Path);
    }
    let path = fs::canonicalize(path).map_err(|_| HostError::Path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Path)?;
    Ok(path)
}

pub(super) fn remove_stale_socket(path: &Path) -> Result<(), HostError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() && trusted_socket_owner(&metadata) => {
            fs::remove_file(path).map_err(|_| HostError::Path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) | Err(_) => Err(HostError::Path),
    }
}

pub(super) fn remove_owned_socket(path: &Path, identity: (u64, u64)) {
    if fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.file_type().is_socket() && (metadata.dev(), metadata.ino()) == identity
    }) {
        drop(fs::remove_file(path));
    }
}

fn trusted_socket_owner(metadata: &fs::Metadata) -> bool {
    metadata.uid() == rustix::process::geteuid().as_raw()
}

pub(super) fn marker_exists(path: &Path) -> Result<bool, HostError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Ok(_) | Err(_) => Err(HostError::Path),
    }
}

pub(in crate::controller_ipc) fn read_drain_token(path: &Path) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 64 {
        return None;
    }
    let mut token = String::new();
    file.take(65).read_to_string(&mut token).ok()?;
    valid_token(&token).then_some(token)
}

pub(in crate::controller_ipc) fn valid_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
