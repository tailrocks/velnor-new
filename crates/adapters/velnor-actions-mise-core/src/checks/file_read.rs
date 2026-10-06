//! Bounded, no-follow reads for named-check source and owned projections.

use crate::MiseError;
use crate::checks::invalid;
use std::fs;
use std::io::Read;
use std::path::Path;

/// Read a regular file through a pinned handle, stopping one byte past its cap.
///
/// # Errors
///
/// Rejects missing files, oversized reads, and expired deadlines.
pub fn read_text(
    path: &Path,
    max_bytes: usize,
    deadline: Option<crate::CheckDeadline>,
    field: &str,
) -> Result<String, MiseError> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| invalid(field, std::io::Error::from(error).to_string()))?;
    if rustix::fs::fstat(&fd)
        .map(|stat| rustix::fs::FileType::from_raw_mode(stat.st_mode))
        .map_err(|error| invalid(field, std::io::Error::from(error).to_string()))?
        != rustix::fs::FileType::RegularFile
    {
        return Err(invalid(field, "not_a_regular_file"));
    }
    let mut file = fs::File::from(fd);
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        checkpoint(deadline, field)?;
        let remaining = max_bytes.saturating_add(1).saturating_sub(bytes.len());
        if remaining == 0 {
            return Err(invalid(field, "oversize"));
        }
        let limit = remaining.min(buffer.len());
        let count = file
            .read(&mut buffer[..limit])
            .map_err(|error| invalid(field, error.to_string()))?;
        checkpoint(deadline, field)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > max_bytes {
            return Err(invalid(field, "oversize"));
        }
    }
    String::from_utf8(bytes).map_err(|error| invalid(field, error.to_string()))
}

fn checkpoint(deadline: Option<crate::CheckDeadline>, field: &str) -> Result<(), MiseError> {
    deadline.map_or(Ok(()), |deadline| {
        deadline
            .remaining()
            .map(|_| ())
            .map_err(|error| invalid(field, error.to_string()))
    })
}

#[cfg(test)]
mod tests;
