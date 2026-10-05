//! Bounded process capture for repository-maintenance probes.

use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const READ_CHUNK: usize = 8 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Captured process result after bounded stdout/stderr reads and a deadline.
pub(crate) struct ProcessOutput {
    /// Final child status.
    pub(crate) status: ExitStatus,
    /// Captured stdout, never larger than the selected cap.
    pub(crate) stdout: Vec<u8>,
    /// Captured stderr, never larger than the selected cap.
    pub(crate) stderr: Vec<u8>,
}

/// Run a command with independently capped streams and a wall-clock timeout.
pub(crate) fn run_bounded(
    command: &mut Command,
    cap: usize,
    timeout: Duration,
) -> Result<ProcessOutput, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("process could not start ({error})"))?;
    let Some(stdout) = child.stdout.take() else {
        stop_child(&mut child);
        return Err("process stdout was unavailable".to_owned());
    };
    let Some(stderr) = child.stderr.take() else {
        stop_child(&mut child);
        return Err("process stderr was unavailable".to_owned());
    };
    let stdout_reader = thread::spawn(move || read_bounded(stdout, cap));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, cap));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                stop_child(&mut child);
                drop(stdout_reader.join());
                drop(stderr_reader.join());
                return Err(format!(
                    "process timed out after {} seconds",
                    timeout.as_secs()
                ));
            }
            Err(error) => {
                stop_child(&mut child);
                drop(stdout_reader.join());
                drop(stderr_reader.join());
                return Err(format!("process status failed ({error})"));
            }
        }
    };
    let stdout = join_reader(stdout_reader, "stdout");
    let stderr = join_reader(stderr_reader, "stderr");
    let (stdout, stderr) = match (stdout, stderr) {
        (Ok(stdout), Ok(stderr)) => (stdout, stderr),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
    })
}

fn stop_child(child: &mut std::process::Child) {
    drop(child.kill());
    drop(child.wait());
}

fn join_reader(
    reader: JoinHandle<Result<Vec<u8>, String>>,
    stream: &str,
) -> Result<Vec<u8>, String> {
    reader
        .join()
        .map_err(|_| format!("process {stream} reader failed"))?
        .map_err(|error| format!("process {stream} {error}"))
}

fn read_bounded(mut reader: impl Read, cap: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(cap)
        .map_err(|error| format!("capture allocation failed ({error})"))?;
    let mut buffer = [0; READ_CHUNK];
    loop {
        let remaining = cap.saturating_sub(bytes.len());
        let limit = remaining.min(buffer.len());
        let count = if limit == 0 {
            reader
                .read(&mut buffer[..1])
                .map_err(|error| format!("output read failed ({error})"))?
        } else {
            reader
                .read(&mut buffer[..limit])
                .map_err(|error| format!("output read failed ({error})"))?
        };
        if count == 0 {
            return Ok(bytes);
        }
        if limit == 0 {
            return Err(format!("output exceeds {cap} bytes"));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::read_bounded;

    #[test]
    fn capture_accepts_exact_limit_and_reads_only_one_extra_byte() {
        let exact = [b'x'; 4];
        assert_eq!(read_bounded(exact.as_slice(), 4), Ok(exact.to_vec()));
        let oversized = [b'x'; 10];
        let mut reader = Cursor::new(oversized);
        assert!(read_bounded(&mut reader, 4).is_err());
        assert_eq!(reader.position(), 5);
    }
}
