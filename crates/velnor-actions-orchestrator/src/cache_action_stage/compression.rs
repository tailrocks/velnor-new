use super::StageError;
use flate2::Compression;
use flate2::bufread::MultiGzDecoder;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub(crate) const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const ZSTD_TIMEOUT: Duration = Duration::from_secs(300);
const PIPE_POLL_INTERVAL: Duration = Duration::from_millis(2);

#[path = "compression_process.rs"]
mod process;
#[cfg(test)]
#[path = "compression_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveCompression {
    None,
    Gzip,
    Zstd,
    ZstdLong,
}

pub(crate) fn decode_archive(
    archive: File,
    compression: ArchiveCompression,
    zstd_path: Option<&Path>,
) -> Result<File, StageError> {
    let metadata = archive
        .metadata()
        .map_err(|_| "cache_tar_archive_open_failed")?;
    if !metadata.is_file() || metadata.len() > MAX_ARCHIVE_BYTES {
        return Err("cache_tar_archive_invalid");
    }
    let mut decoded = tempfile::tempfile().map_err(|_| "cache_tar_temporary_file_failed")?;
    match compression {
        ArchiveCompression::None => copy_bounded(archive, &mut decoded)?,
        ArchiveCompression::Gzip => {
            let mut reader = MultiGzDecoder::new(BufReader::new(archive));
            copy_bounded(&mut reader, &mut decoded).map_err(|_| "cache_tar_compression_failed")?;
            let mut source = reader.into_inner();
            if !source
                .fill_buf()
                .map_err(|_| "cache_tar_compression_failed")?
                .is_empty()
                || source
                    .stream_position()
                    .map_err(|_| "cache_tar_compression_failed")?
                    != metadata.len()
            {
                return Err("cache_tar_compression_trailing_data");
            }
        }
        ArchiveCompression::Zstd | ArchiveCompression::ZstdLong => {
            let program = checked_zstd(zstd_path)?;
            let long_mode = if compression == ArchiveCompression::ZstdLong {
                "--long=30"
            } else {
                "--no-progress"
            };
            process::run_filter(
                Command::new(program).args(["-d", "-q", "-c", long_mode]),
                archive,
                &mut decoded,
                MAX_ARCHIVE_BYTES,
                MAX_ARCHIVE_BYTES,
                ZSTD_TIMEOUT,
            )?;
        }
    }
    decoded
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_temporary_file_failed")?;
    Ok(decoded)
}

pub(crate) fn encode_archive(
    mut raw: File,
    output: &mut File,
    compression: ArchiveCompression,
    zstd_path: Option<&Path>,
) -> Result<(), StageError> {
    let length = raw
        .metadata()
        .map_err(|_| "cache_tar_temporary_file_failed")?
        .len();
    if length > MAX_ARCHIVE_BYTES {
        return Err("cache_tar_archive_limit");
    }
    match compression {
        ArchiveCompression::None => copy_bounded(raw, output),
        ArchiveCompression::Gzip => {
            let mut encoder = GzEncoder::new(output, Compression::default());
            copy_bounded(raw, &mut encoder)?;
            encoder
                .finish()
                .map(|_| ())
                .map_err(|_| "cache_tar_archive_write_failed")
        }
        ArchiveCompression::Zstd | ArchiveCompression::ZstdLong => {
            let program = checked_zstd(zstd_path)?;
            raw.seek(SeekFrom::Start(0))
                .map_err(|_| "cache_tar_temporary_file_failed")?;
            let long_mode = if compression == ArchiveCompression::ZstdLong {
                "--long=30"
            } else {
                "--no-progress"
            };
            process::run_filter(
                Command::new(program).args(["-q", "-T0", "-c", long_mode]),
                raw,
                output,
                MAX_ARCHIVE_BYTES,
                MAX_ARCHIVE_BYTES,
                ZSTD_TIMEOUT,
            )
        }
    }
}

fn checked_zstd(path: Option<&Path>) -> Result<&Path, StageError> {
    let path = path.ok_or("cache_tar_compression_unavailable")?;
    if !path.is_absolute() || path.as_os_str().len() > 4_096 {
        return Err("cache_tar_compression_unavailable");
    }
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "cache_tar_compression_unavailable")?;
    if !metadata.file_type().is_file() {
        return Err("cache_tar_compression_unavailable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("cache_tar_compression_unavailable");
        }
    }
    Ok(path)
}

fn copy_bounded<R: Read, W: Write>(mut reader: R, writer: &mut W) -> Result<(), StageError> {
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| "cache_tar_archive_read_failed")?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| "cache_tar_archive_limit")?)
            .ok_or("cache_tar_archive_limit")?;
        if total > MAX_ARCHIVE_BYTES {
            return Err("cache_tar_archive_limit");
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|_| "cache_tar_archive_write_failed")?;
    }
    Ok(())
}
