use std::fs::File;
use std::io::Read;

use rustix::fs::statvfs;

use crate::memory::available_bytes;
use crate::pressure::memory_some_avg10;
use crate::{ProbeError, ProbeRecord, load};

const DOCKER_ROOT: &str = "/velnor/docker-root";
const MEMINFO_PATH: &str = "/proc/meminfo";
const LOADAVG_PATH: &str = "/proc/loadavg";
const PRESSURE_PATH: &str = "/proc/pressure/memory";
const MEMINFO_LIMIT: usize = 16 * 1024;
const LOADAVG_LIMIT: usize = 256;
const PRESSURE_LIMIT: usize = 4096;

/// Read the fixed, bounded measurement sources into one versioned record.
///
/// # Errors
///
/// Returns an error when any mandatory measurement is missing, malformed, too
/// large, or outside the output range.
pub fn sample() -> Result<ProbeRecord, ProbeError> {
    let root =
        statvfs(DOCKER_ROOT).map_err(|error| ProbeError::Read("docker_root", error.into()))?;
    let docker_root_free_bytes = root
        .f_bavail
        .checked_mul(root.f_frsize)
        .ok_or(ProbeError::Overflow("docker_root"))?;
    let memory_available_bytes =
        available_bytes(&read_bounded(MEMINFO_PATH, MEMINFO_LIMIT, "meminfo")?)?;
    let load_milli =
        load::one_minute_milli(&read_bounded(LOADAVG_PATH, LOADAVG_LIMIT, "loadavg")?)?;
    let memory_psi_some_avg10_bps = read_bounded(PRESSURE_PATH, PRESSURE_LIMIT, "psi")
        .ok()
        .and_then(|bytes| memory_some_avg10(&bytes));
    Ok(ProbeRecord {
        schema_version: 1,
        docker_root_free_bytes,
        memory_available_bytes,
        load_milli,
        memory_psi_some_avg10_bps,
    })
}

fn read_bounded(path: &str, limit: usize, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    let file = File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take(u64::try_from(limit + 1).map_err(|_| ProbeError::Overflow("input_limit"))?)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}
