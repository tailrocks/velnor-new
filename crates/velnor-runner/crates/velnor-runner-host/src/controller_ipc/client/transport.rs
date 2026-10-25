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
    let response_limit = request.response_limit();
    let request_bytes = request.encode().map_err(|()| HostError::Frame)?;
    write_all_until(&mut stream, &encode_frame(&request_bytes)?, deadline)?;

    let mut prefix = [0; 4];
    read_exact_until(&mut stream, &mut prefix, deadline)?;
    let length = usize::try_from(u32::from_be_bytes(prefix)).map_err(|_| HostError::Frame)?;
    if length == 0 || length > response_limit || length > MAX_FRAME {
        return Err(HostError::Frame);
    }
    let mut bytes = vec![0; length];
    read_exact_until(&mut stream, &mut bytes, deadline)?;
    Response::decode_with_limit(&bytes, response_limit).map_err(|()| HostError::Frame)
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::thread;

    use super::*;
    use crate::config::ConnectionId;
    use crate::controller_ipc::wire::{
        ConnectionSetupSnapshot, Request, Response, RunnerGroupSnapshot, SelectedSetSnapshot,
        SetupReceiptReply, SetupReceiptStatus,
    };

    #[test]
    fn setup_status_transport_roundtrips_responses_over_the_legacy_limit() {
        let (directory, path, listener) = local_socket();
        let response = setup_response(800);
        let bytes = serde_json::to_vec(&response).expect("test response serializes");
        assert!(bytes.len() > 4096);
        assert!(bytes.len() <= MAX_FRAME);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("client connects");
            let framed = crate::ipc::encode_frame(&bytes).expect("response frame fits");
            stream.write_all(&framed).expect("response is written");
        });
        let request = setup_request();
        assert_eq!(request.response_limit(), MAX_FRAME);
        assert_eq!(
            Request::DoctorProbeStatus {
                nonce: "0123456789abcdef0123456789abcdef".to_owned()
            }
            .response_limit(),
            4096
        );
        let decoded = call_socket_response_with_timeout(&path, &request, Duration::from_secs(3))
            .expect("new setup response limit accepts the complete response");
        server.join().expect("server thread succeeds");
        let groups = decoded
            .setup_receipt
            .expect("setup receipt is present")
            .setup
            .expect("setup snapshot is present")
            .runner_groups;
        assert_eq!(groups.len(), 800);
        fs::remove_dir_all(directory).expect("temporary socket directory is removed");
    }

    #[test]
    fn setup_status_transport_rejects_oversized_header_before_body_read() {
        let (directory, path, listener) = local_socket();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("client connects");
            let oversized = u32::try_from(MAX_FRAME + 1)
                .expect("test frame limit fits the protocol prefix")
                .to_be_bytes();
            stream
                .write_all(&oversized)
                .expect("oversized header is written");
        });
        let result =
            call_socket_response_with_timeout(&path, &setup_request(), Duration::from_secs(3));
        assert!(matches!(result, Err(HostError::Frame)));
        server.join().expect("server thread succeeds");
        fs::remove_dir_all(directory).expect("temporary socket directory is removed");
    }

    fn setup_request() -> Request {
        Request::ConnectionSetupStatus {
            connection_id: ConnectionId::parse("essential").expect("test id is valid"),
        }
    }

    fn setup_response(group_count: usize) -> Response {
        Response {
            value: None,
            doctor_probe: None,
            readiness: None,
            session_observation: None,
            setup_receipt: Some(SetupReceiptReply {
                status: SetupReceiptStatus::Ready,
                setup: Some(ConnectionSetupSnapshot {
                    revision: 1,
                    captured_at_unix_ms: 1_700_000_000_000,
                    runner_groups: (0..group_count)
                        .map(|index| RunnerGroupSnapshot {
                            id: i64::try_from(index + 1).expect("test group id fits"),
                            name: format!("group-{index:04}"),
                            is_default: index == 0,
                        })
                        .collect(),
                    selected_set: SelectedSetSnapshot {
                        id: 17,
                        name: "verified-set".to_owned(),
                        runner_group_id: 1,
                        disable_update: true,
                        labels: vec!["self-hosted".to_owned()],
                    },
                }),
            }),
            connection_census: None,
        }
    }

    fn local_socket() -> (std::path::PathBuf, std::path::PathBuf, UnixListener) {
        let directory = std::path::Path::new("/tmp").join(format!(
            "vipc-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("temporary socket directory is created");
        let path = directory.join("s");
        let listener = UnixListener::bind(&path).expect("test socket binds");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("test socket permissions are private");
        (directory, path, listener)
    }
}
