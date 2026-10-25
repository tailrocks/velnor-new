use std::path::Path;

use crate::error::HostError;
use crate::resource_probe_record::{MAX_OUTPUT_BYTES, ProbeRecord};

pub(super) fn validate(output: &Path, root: &Path) -> Result<(), HostError> {
    if !root.is_dir() {
        return Err(HostError::Path);
    }
    let bytes = super::files::read_bounded_regular(output, MAX_OUTPUT_BYTES)?;
    ProbeRecord::parse_schema5(&bytes).ok_or(HostError::Identity)?;
    Ok(())
}

#[cfg(test)]
#[path = "schema5_tests.rs"]
mod tests;
