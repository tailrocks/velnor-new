//! Write one bounded version-3 guest measurement record to stdout.

mod main_ack;
mod main_commands;

use std::{
    io::{self, Write},
    thread,
    time::Duration,
};

use velnor_resource_probe::{
    MAX_OUTPUT_BYTES, ProbeError, ProbeRecord, sample, worker_binding::WORKER_RECORD_LIMIT,
};

#[cfg(target_os = "linux")]
use velnor_resource_probe::worker_binding::read_compact_line;

use main_commands::{
    run_cleanup_observation, run_cleanup_observer, run_execution_channel,
    run_execution_publication, run_execution_request, run_job_facts_publication, run_owned_cgroup,
    run_self_storage,
};

#[cfg(test)]
pub(crate) use main_ack::{AckRead, wait_for_ack_with};

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
        "execution_channel" => run_execution_channel(),
        "execution_request" => run_execution_request(),
        "execution_publication" => run_execution_publication(),
        "job_facts_publication" => run_job_facts_publication(),
        "cleanup_observation" => run_cleanup_observation(),
        "cleanup_observer" => run_cleanup_observer(),
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
    write_worker_record_limit(record, output, WORKER_RECORD_LIMIT)
}

fn write_worker_record_limit(
    record: &impl serde::Serialize,
    output: &mut impl Write,
    limit: usize,
) -> Result<(), ProbeError> {
    let mut bytes = serde_json::to_vec(record).map_err(|_| ProbeError::Invalid("json_encode"))?;
    bytes.push(b'\n');
    if bytes.len() > limit {
        return Err(ProbeError::OutputTooLarge);
    }
    output
        .write_all(&bytes)
        .and_then(|()| output.flush())
        .map_err(|_| ProbeError::Invalid("stdout_write"))
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
