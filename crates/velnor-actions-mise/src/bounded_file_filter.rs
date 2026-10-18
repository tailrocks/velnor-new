//! Runs bounded compression filters through the isolated process layer.
use crate::command::command_for_file_filter;
use std::fs::File;
use std::path::Path;
use std::time::Duration;

/// Error code returned by the bounded file-filter executor.
pub type FileFilterError = &'static str;

/// Fixed zstd operations supported by archive adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZstdFilterMode {
    /// Decode a standard zstd stream.
    Decode,
    /// Decode a long-window zstd stream.
    DecodeLong,
    /// Encode a standard zstd stream.
    Encode,
    /// Encode a long-window zstd stream.
    EncodeLong,
}

/// Runs zstd with regular-file streams, byte limits, and a deadline.
/// The deadline applies to child execution. File operations can continue after this deadline.
///
/// # Errors
/// Returns an error code when a file, filter, stream, limit, or deadline fails.
pub fn run_zstd_file_filter(
    program: &Path,
    mode: ZstdFilterMode,
    input: File,
    output: &mut File,
    input_limit: u64,
    output_limit: u64,
    timeout: Duration,
) -> Result<(), FileFilterError> {
    let input_metadata = input
        .metadata()
        .map_err(|_| "cache_tar_archive_read_failed")?;
    let output_metadata = output
        .metadata()
        .map_err(|_| "cache_tar_archive_write_failed")?;
    if !input_metadata.is_file() || !output_metadata.is_file() {
        return Err("cache_tar_archive_invalid");
    }
    if !program.is_absolute() || program.as_os_str().len() > 4_096 {
        return Err("cache_tar_compression_unavailable");
    }
    let program_metadata =
        std::fs::symlink_metadata(program).map_err(|_| "cache_tar_compression_unavailable")?;
    if !program_metadata.file_type().is_file() {
        return Err("cache_tar_compression_unavailable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if program_metadata.permissions().mode() & 0o111 == 0 {
            return Err("cache_tar_compression_unavailable");
        }
    }
    let arguments = match mode {
        ZstdFilterMode::Decode => ["-d", "-q", "-c", "--no-progress"],
        ZstdFilterMode::DecodeLong => ["-d", "-q", "-c", "--long=30"],
        ZstdFilterMode::Encode => ["-q", "-T0", "-c", "--no-progress"],
        ZstdFilterMode::EncodeLong => ["-q", "-T0", "-c", "--long=30"],
    };
    let mut command = command_for_file_filter(program.as_os_str(), &arguments);
    process::run_filter_io(
        &mut command,
        input,
        output,
        input_limit,
        output_limit,
        timeout,
    )
}

#[path = "bounded_file_filter_process.rs"]
mod process;

#[cfg(test)]
#[path = "../tests/bounded_file_filter_unit.rs"]
mod tests;
