//! This utility sets the owner and mode of the Runner home volume and Work volume for one job.

mod storage_init;

#[cfg(target_os = "linux")]
use std::io::Write;
use std::io::{self, Read};

const INPUT_LIMIT: usize = 256;
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
    let mut input = Vec::with_capacity(INPUT_LIMIT);
    io::stdin()
        .take((INPUT_LIMIT + 1) as u64)
        .read_to_end(&mut input)
        .map_err(|_| "input_read")?;
    if input.len() > INPUT_LIMIT {
        return Err("input_limit");
    }
    let request: storage_init::Request = serde_json::from_slice(&input).map_err(|_| "request")?;
    if !storage_init::valid_operation_id(&request.operation_id) {
        return Err("operation_id");
    }
    #[cfg(target_os = "linux")]
    {
        let operation_id = request.operation_id;
        let result = match storage_init::initialize(operation_id.clone()) {
            Ok(record) => record,
            Err(error) => {
                let failure = storage_init::FailureRecord {
                    schema: 1,
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
        io::stdout()
            .lock()
            .write_all(&output)
            .map_err(|_| "result_write")
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = request;
        Err("linux_required")
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
