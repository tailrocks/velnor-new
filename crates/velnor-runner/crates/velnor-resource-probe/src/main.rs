//! Write one bounded version-1 guest measurement record to stdout.

use std::io::{self, Write};

use velnor_resource_probe::{MAX_OUTPUT_BYTES, ProbeError, ProbeRecord, sample};

fn main() {
    if let Err(error) = run() {
        eprintln!("resource probe: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), ProbeError> {
    let record = sample()?;
    write_record(&record, &mut io::stdout().lock())
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
