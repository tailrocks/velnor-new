use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::HostError;

const MAX_OUTPUT_BYTES: usize = 512;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProbeRecord {
    schema_version: u8,
    docker_root_free_bytes: u64,
    docker_root_total_bytes: u64,
    memory_available_bytes: u64,
    load_milli: u64,
    memory_psi_some_avg10_bps: Option<u64>,
}

pub(super) fn validate(output: &Path, root: &Path) -> Result<(), HostError> {
    if !root.is_dir() {
        return Err(HostError::Path);
    }
    let bytes = read_output(output)?;
    let record = parse_record(&bytes)?;
    validate_record(&record)?;
    Ok(())
}

fn read_output(path: &Path) -> Result<Vec<u8>, HostError> {
    super::files::read_bounded_regular(path, MAX_OUTPUT_BYTES)
}

fn parse_record(bytes: &[u8]) -> Result<ProbeRecord, HostError> {
    if bytes.is_empty() || !bytes.ends_with(b"\n") || bytes[..bytes.len() - 1].contains(&b'\n') {
        return Err(HostError::Identity);
    }
    let value = crate::artifact_admission::parse_release_json(&bytes[..bytes.len() - 1])?;
    let record: ProbeRecord = serde_json::from_value(value).map_err(|_| HostError::Identity)?;
    let mut canonical = serde_json::to_vec(&record).map_err(|_| HostError::Identity)?;
    canonical.push(b'\n');
    if bytes != canonical {
        return Err(HostError::Identity);
    }
    Ok(record)
}

fn validate_record(record: &ProbeRecord) -> Result<(), HostError> {
    if record.schema_version != 1
        || record.docker_root_free_bytes > record.docker_root_total_bytes
        || record
            .memory_psi_some_avg10_bps
            .is_some_and(|value| value > 10_000)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}
