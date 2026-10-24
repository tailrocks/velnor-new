#[cfg(any(target_os = "linux", test))]
use std::time::Duration;

use velnor_resource_probe::ProbeError;

#[cfg(any(target_os = "linux", test))]
use velnor_resource_probe::worker_binding::{WORKER_ACK_LIMIT, validate_acknowledgment};

#[cfg(target_os = "linux")]
use velnor_resource_probe::worker_binding::ACK_TIMEOUT_SECONDS;

#[cfg(target_os = "linux")]
pub(super) fn wait_for_ack(operation_id: &str) -> Result<(), ProbeError> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use std::{io, os::fd::AsFd, time::Instant};

    let stdin = io::stdin();
    let fd = stdin.as_fd();
    let started = Instant::now();
    wait_for_ack_with(
        operation_id,
        Duration::from_secs(ACK_TIMEOUT_SECONDS),
        || started.elapsed(),
        |remaining, buffer| {
            loop {
                let timeout = Timespec {
                    tv_sec: i64::try_from(remaining.as_secs())
                        .map_err(|_| ProbeError::Overflow("ack_timeout"))?,
                    tv_nsec: i64::from(remaining.subsec_nanos()),
                };
                let mut pollfds = [PollFd::new(&fd, PollFlags::IN)];
                if poll(&mut pollfds, Some(&timeout))
                    .map_err(|_| ProbeError::Invalid("ack_poll"))?
                    == 0
                {
                    return Ok(AckRead::Timeout);
                }
                match rustix::io::read(fd, &mut *buffer) {
                    Ok(0) => return Ok(AckRead::Eof),
                    Ok(count) => return Ok(AckRead::Data(count)),
                    Err(rustix::io::Errno::INTR) if remaining.is_zero() => {
                        return Ok(AckRead::Timeout);
                    }
                    Err(rustix::io::Errno::INTR) => {}
                    Err(_) => return Err(ProbeError::Invalid("ack_read")),
                }
            }
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(any(target_os = "linux", test))]
pub(super) enum AckRead {
    Data(usize),
    Eof,
    Timeout,
}

#[cfg(any(target_os = "linux", test))]
/// Read one bounded acknowledgment frame before its deadline.
///
/// The newline ends the single ACK frame. Send no later bytes. The reader does not require EOF.
pub(super) fn wait_for_ack_with(
    operation_id: &str,
    timeout: Duration,
    mut elapsed: impl FnMut() -> Duration,
    mut read_ready: impl FnMut(Duration, &mut [u8]) -> Result<AckRead, ProbeError>,
) -> Result<(), ProbeError> {
    let mut bytes = Vec::with_capacity(WORKER_ACK_LIMIT);
    let mut buffer = [0_u8; WORKER_ACK_LIMIT + 1];
    let start = elapsed();
    loop {
        let spent = elapsed()
            .checked_sub(start)
            .ok_or(ProbeError::Invalid("ack_clock"))?;
        let remaining = timeout
            .checked_sub(spent)
            .filter(|duration| !duration.is_zero())
            .ok_or(ProbeError::Invalid("ack_timeout"))?;
        let read = read_ready(remaining, &mut buffer)?;
        let spent = elapsed()
            .checked_sub(start)
            .ok_or(ProbeError::Invalid("ack_clock"))?;
        if spent >= timeout {
            return Err(ProbeError::Invalid("ack_timeout"));
        }
        match read {
            AckRead::Timeout => return Err(ProbeError::Invalid("ack_timeout")),
            AckRead::Eof => return Err(ProbeError::Invalid("ack_line_missing")),
            AckRead::Data(count) if count == 0 || count > buffer.len() => {
                return Err(ProbeError::Invalid("ack_read"));
            }
            AckRead::Data(count) => {
                let chunk = &buffer[..count];
                if let Some(newline) = chunk.iter().position(|byte| *byte == b'\n') {
                    if newline + 1 != count {
                        return Err(ProbeError::Invalid("ack_extra_data"));
                    }
                    if bytes.len().saturating_add(count) > WORKER_ACK_LIMIT {
                        return Err(ProbeError::InputTooLarge("ack"));
                    }
                    bytes.extend_from_slice(chunk);
                    validate_acknowledgment(&bytes, operation_id)?;
                    return match read_ready(Duration::ZERO, &mut buffer)? {
                        AckRead::Data(count) if count > 0 => {
                            Err(ProbeError::Invalid("ack_extra_data"))
                        }
                        AckRead::Data(_) => Err(ProbeError::Invalid("ack_read")),
                        AckRead::Eof | AckRead::Timeout => Ok(()),
                    };
                }
                if bytes.len().saturating_add(count) > WORKER_ACK_LIMIT {
                    return Err(ProbeError::InputTooLarge("ack"));
                }
                bytes.extend_from_slice(chunk);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn wait_for_ack(_operation_id: &str) -> Result<(), ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}
