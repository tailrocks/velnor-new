//! This program sets the owner and mode of specified volumes and directories for one job.

mod storage_init;

#[cfg(target_os = "linux")]
use std::io::Write;
use std::io::{self, Read};

const INPUT_LIMIT: usize = 256;
#[cfg(target_os = "linux")]
const ACK_LIMIT: usize = 256;
#[cfg(target_os = "linux")]
const OUTPUT_LIMIT: usize = 2048;

fn main() {
    if let Err(error) = run() {
        eprintln!("Storage initialization failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), &'static str> {
    if std::env::args_os().len() != 1 {
        return Err("arguments");
    }
    let mut stdin = io::stdin().lock();
    let request_line = read_line(&mut stdin, INPUT_LIMIT, "input_limit")?;
    let request_bytes = request_line.strip_suffix(b"\n").ok_or("request_line")?;
    let request: storage_init::Request =
        serde_json::from_slice(request_bytes).map_err(|_| "request")?;
    if serde_json::to_vec(&request).map_err(|_| "request_encode")? != request_bytes {
        return Err("request_canonical");
    }
    if !storage_init::valid_operation_id(&request.operation_id) {
        return Err("operation_id");
    }
    #[cfg(target_os = "linux")]
    {
        let operation_id = request.operation_id.clone();
        let result = match storage_init::initialize(operation_id.clone()) {
            Ok(record) => record,
            Err(error) => {
                let failure = storage_init::FailureRecord {
                    schema: 2,
                    operation_id,
                    status: "failed",
                    error,
                };
                write_record(&failure)?;
                return Err("initialization_failed");
            }
        };
        let mut output = serde_json::to_vec(&result).map_err(|_| "result_encode")?;
        output.push(b'\n');
        if output.len() > OUTPUT_LIMIT {
            return Err("result_limit");
        }
        let mut stdout = io::stdout().lock();
        stdout.write_all(&output).map_err(|_| "result_write")?;
        stdout.flush().map_err(|_| "result_flush")?;

        let acknowledgment_line = read_line(&mut stdin, ACK_LIMIT, "acknowledgment_limit")?;
        let acknowledgment_bytes = acknowledgment_line
            .strip_suffix(b"\n")
            .ok_or("acknowledgment_line")?;
        let acknowledgment: storage_init::Acknowledgment =
            serde_json::from_slice(acknowledgment_bytes).map_err(|_| "acknowledgment")?;
        if serde_json::to_vec(&acknowledgment).map_err(|_| "acknowledgment_encode")?
            != acknowledgment_bytes
        {
            return Err("acknowledgment_canonical");
        }
        storage_init::validate_acknowledgment(&acknowledgment, &request.operation_id)?;
        let mut trailing = [0_u8; 1];
        if stdin
            .read(&mut trailing)
            .map_err(|_| "acknowledgment_read")?
            != 0
        {
            return Err("acknowledgment_trailing_data");
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = request;
        Err("linux_required")
    }
}

fn read_line(
    input: &mut impl Read,
    limit: usize,
    limit_error: &'static str,
) -> Result<Vec<u8>, &'static str> {
    let mut line = Vec::with_capacity(limit.min(128));
    let mut byte = [0_u8; 1];
    loop {
        let count = input.read(&mut byte).map_err(|_| "input_read")?;
        if count == 0 {
            return Err("input_eof");
        }
        line.push(byte[0]);
        if line.len() > limit {
            return Err(limit_error);
        }
        if byte[0] == b'\n' {
            return Ok(line);
        }
    }
}

#[cfg(target_os = "linux")]
fn write_record(record: &impl serde::Serialize) -> Result<(), &'static str> {
    let mut output = serde_json::to_vec(record).map_err(|_| "result_encode")?;
    output.push(b'\n');
    if output.len() > OUTPUT_LIMIT {
        return Err("result_limit");
    }
    io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|_| "result_write")
}
