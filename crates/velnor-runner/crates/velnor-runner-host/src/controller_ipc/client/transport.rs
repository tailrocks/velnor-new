use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::HostError;
use crate::ipc::{MAX_FRAME, encode_frame};

use super::super::wire::{Request, Response};

pub(super) fn call_socket_response_with_timeout(
    path: &Path,
    request: &Request,
    timeout: Duration,
) -> Result<Response, HostError> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(HostError::Frame)?;
    validate_socket(path)?;
    let mut stream = UnixStream::connect(path).map_err(|_| HostError::Lock)?;
    stream.set_nonblocking(true).map_err(|_| HostError::Frame)?;
    let request = request.encode().map_err(|()| HostError::Frame)?;
    write_all_until(&mut stream, &encode_frame(&request)?, deadline)?;

    let mut prefix = [0; 4];
    read_exact_until(&mut stream, &mut prefix, deadline)?;
    let length = usize::try_from(u32::from_be_bytes(prefix)).map_err(|_| HostError::Frame)?;
    if length == 0 || length > 4096 || length > MAX_FRAME {
        return Err(HostError::Frame);
    }
    let mut bytes = vec![0; length];
    read_exact_until(&mut stream, &mut bytes, deadline)?;
    Response::decode(&bytes).map_err(|()| HostError::Frame)
}

fn write_all_until(
    stream: &mut UnixStream,
    bytes: &[u8],
    deadline: Instant,
) -> Result<(), HostError> {
    let mut offset = 0;
    while offset < bytes.len() {
        wait_for_io(stream, rustix::event::PollFlags::OUT, deadline)?;
        match stream.write(&bytes[offset..]) {
            Ok(0) => return Err(HostError::Frame),
            Ok(written) => offset += written,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(_) => return Err(HostError::Frame),
        }
    }
    Ok(())
}

fn read_exact_until(
    stream: &mut UnixStream,
    bytes: &mut [u8],
    deadline: Instant,
) -> Result<(), HostError> {
    let mut offset = 0;
    while offset < bytes.len() {
        wait_for_io(stream, rustix::event::PollFlags::IN, deadline)?;
        match stream.read(&mut bytes[offset..]) {
            Ok(0) => return Err(HostError::Frame),
            Ok(read) => offset += read,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(_) => return Err(HostError::Frame),
        }
    }
    Ok(())
}

fn wait_for_io(
    stream: &UnixStream,
    flags: rustix::event::PollFlags,
    deadline: Instant,
) -> Result<(), HostError> {
    use rustix::event::{PollFd, Timespec, poll};

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HostError::Frame);
        }
        let timeout = Timespec {
            tv_sec: i64::try_from(remaining.as_secs()).map_err(|_| HostError::Frame)?,
            tv_nsec: i64::from(remaining.subsec_nanos()),
        };
        let mut descriptors = [PollFd::new(stream, flags)];
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => return Err(HostError::Frame),
            Ok(_) if Instant::now() < deadline => return Ok(()),
            Err(rustix::io::Errno::INTR) => {}
            Ok(_) | Err(_) => return Err(HostError::Frame),
        }
    }
}

fn validate_socket(path: &Path) -> Result<(), HostError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Lock)?;
    let owner = rustix::process::geteuid().as_raw();
    if !metadata.file_type().is_socket()
        || metadata.uid() != owner
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(HostError::Path);
    }
    Ok(())
}
