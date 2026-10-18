//! Strict end-of-stream admission for a single gzip-compressed tar archive.

use std::io::Read;
use std::time::Instant;

use crate::OrchestratorError;

use super::{MAX_TOTAL_DECOMPRESSED_BYTES, check_deadline, invalid};

/// Drain and validate the remaining zero blocks through gzip EOF.
///
/// Reading to EOF validates the gzip checksum and size trailer. The caller then
/// checks the underlying compressed reader for a second member or trailing data.
pub(super) fn drain_tar_terminator<R: Read>(
    decoded: &mut R,
    decompressed_total: &mut u64,
    started: Instant,
) -> Result<(), OrchestratorError> {
    let mut buffer = [0u8; 8192];
    let mut trailer_size = 0u64;
    loop {
        check_deadline(started)?;
        let read = decoded
            .read(&mut buffer)
            .map_err(|_| invalid("preview_gzip_stream_invalid"))?;
        if read == 0 {
            break;
        }
        if buffer[..read].iter().any(|byte| *byte != 0) {
            return Err(invalid("preview_tar_nonzero_trailer"));
        }
        trailer_size = trailer_size.saturating_add(read as u64);
        *decompressed_total = decompressed_total.saturating_add(read as u64);
        if *decompressed_total > MAX_TOTAL_DECOMPRESSED_BYTES {
            return Err(invalid("preview_tar_decompression_limit"));
        }
    }
    if trailer_size < 512 || !trailer_size.is_multiple_of(512) {
        return Err(invalid("preview_tar_terminator_invalid"));
    }
    Ok(())
}
