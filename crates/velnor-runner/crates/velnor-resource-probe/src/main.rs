//! Write one bounded version-3 guest measurement record to stdout.

use std::{
    io::{self, Write},
    thread,
    time::Duration,
};

use velnor_resource_probe::{
    MAX_OUTPUT_BYTES, ProbeError, ProbeRecord, sample,
    worker_binding::{
        OwnedCgroupRequest, SelfStorageRequest, WORKER_RECORD_LIMIT, WORKER_REQUEST_LIMIT,
        observe_owned_cgroups, observe_self_storage,
    },
};

#[cfg(any(target_os = "linux", test))]
use velnor_resource_probe::worker_binding::{WORKER_ACK_LIMIT, validate_acknowledgment};

#[cfg(target_os = "linux")]
use velnor_resource_probe::worker_binding::ACK_TIMEOUT_SECONDS;

#[cfg(target_os = "linux")]
use velnor_resource_probe::worker_binding::read_compact_line;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = run(&arguments) {
        eprintln!("resource probe: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: &[String]) -> Result<(), ProbeError> {
    if arguments.is_empty() {
        return run_capacity();
    }
    let [flag, mode] = arguments else {
        return Err(ProbeError::Invalid("arguments"));
    };
    if flag != "--mode" {
        return Err(ProbeError::Invalid("arguments"));
    }
    match mode.as_str() {
        "self_storage" => run_self_storage(),
        "owned_container_cgroup" => run_owned_cgroup(),
        _ => Err(ProbeError::Invalid("worker_mode")),
    }
}

fn run_capacity() -> Result<(), ProbeError> {
    let record = sample()?;
    // Keep the trusted process inspectable long enough to bind its NSpid host ID.
    thread::sleep(Duration::from_secs(1));
    write_record(&record, &mut io::stdout().lock())
}

fn run_self_storage() -> Result<(), ProbeError> {
    let request: SelfStorageRequest = read_worker_request(WORKER_REQUEST_LIMIT)?;
    let operation_id = request.operation_id.clone();
    let record = observe_self_storage(request)?;
    write_worker_record(&record, &mut io::stdout().lock())?;
    wait_for_ack(&operation_id)
}

fn run_owned_cgroup() -> Result<(), ProbeError> {
    let request: OwnedCgroupRequest = read_worker_request(WORKER_REQUEST_LIMIT)?;
    let record = observe_owned_cgroups(request)?;
    write_worker_record(&record, &mut io::stdout().lock())
}

#[cfg(target_os = "linux")]
fn read_worker_request<T>(limit: usize) -> Result<T, ProbeError>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    use std::os::fd::AsFd;

    let stdin = io::stdin();
    let fd = stdin.as_fd();
    let mut bytes = Vec::with_capacity(limit.min(4096));
    let mut byte = [0_u8; 1];
    loop {
        match rustix::io::read(fd, &mut byte) {
            Ok(0) => return Err(ProbeError::Invalid("worker_line_missing")),
            Ok(1) if byte[0] == b'\n' => {
                bytes.push(b'\n');
                return read_compact_line(&mut bytes.as_slice(), limit);
            }
            Ok(1) => {
                bytes.push(byte[0]);
                if bytes.len() > limit {
                    return Err(ProbeError::InputTooLarge("worker_line"));
                }
            }
            Err(rustix::io::Errno::INTR) => {}
            Ok(_) | Err(_) => return Err(ProbeError::Invalid("worker_read")),
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn read_worker_request<T>(_limit: usize) -> Result<T, ProbeError>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    Err(ProbeError::Invalid("linux_guest_required"))
}

fn write_worker_record(
    record: &impl serde::Serialize,
    output: &mut impl Write,
) -> Result<(), ProbeError> {
    let mut bytes = serde_json::to_vec(record).map_err(|_| ProbeError::Invalid("json_encode"))?;
    bytes.push(b'\n');
    if bytes.len() > WORKER_RECORD_LIMIT {
        return Err(ProbeError::OutputTooLarge);
    }
    output
        .write_all(&bytes)
        .and_then(|()| output.flush())
        .map_err(|_| ProbeError::Invalid("stdout_write"))
}

#[cfg(target_os = "linux")]
fn wait_for_ack(operation_id: &str) -> Result<(), ProbeError> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use std::{os::fd::AsFd, time::Instant};

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
enum AckRead {
    Data(usize),
    Eof,
    Timeout,
}

#[cfg(any(target_os = "linux", test))]
/// Read one bounded acknowledgment frame before its deadline.
///
/// The newline ends the single ACK frame. Send no later bytes. The reader does not require EOF.
fn wait_for_ack_with(
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
fn wait_for_ack(_operation_id: &str) -> Result<(), ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

fn write_record(record: &ProbeRecord, output: &mut impl Write) -> Result<(), ProbeError> {
    let mut bytes = serde_json::to_vec(record).map_err(|_| ProbeError::Invalid("json_encode"))?;
    bytes.push(b'\n');
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(ProbeError::OutputTooLarge);
    }
    output
        .write_all(&bytes)
        .map_err(|_| ProbeError::Invalid("stdout_write"))
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
