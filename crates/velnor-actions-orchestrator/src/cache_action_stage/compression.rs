use super::StageError;
use flate2::Compression;
use flate2::bufread::MultiGzDecoder;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Command, Stdio};

pub(crate) const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024 * 1024;

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
            let mut child = Command::new(program)
                .args([
                    "-d",
                    "-q",
                    "-c",
                    if compression == ArchiveCompression::ZstdLong {
                        "--long=30"
                    } else {
                        "--no-progress"
                    },
                ])
                .stdin(Stdio::from(archive))
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "cache_tar_compression_failed")?;
            let mut output = child.stdout.take().ok_or("cache_tar_compression_failed")?;
            copy_bounded(&mut output, &mut decoded)?;
            let status = child.wait().map_err(|_| "cache_tar_compression_failed")?;
            if !status.success() {
                return Err("cache_tar_compression_failed");
            }
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
            let mut child = Command::new(program)
                .args([
                    "-q",
                    "-T0",
                    "-c",
                    if compression == ArchiveCompression::ZstdLong {
                        "--long=30"
                    } else {
                        "--no-progress"
                    },
                ])
                .stdin(Stdio::from(raw))
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "cache_tar_compression_failed")?;
            let mut source = child.stdout.take().ok_or("cache_tar_compression_failed")?;
            copy_bounded(&mut source, output)?;
            let status = child.wait().map_err(|_| "cache_tar_compression_failed")?;
            if !status.success() {
                return Err("cache_tar_compression_failed");
            }
            Ok(())
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

#[cfg(test)]
mod tests {
    use super::{ArchiveCompression, decode_archive};
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::{Read, Seek, Write};

    fn gzip(input: &[u8]) -> std::fs::File {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(input).expect("write gzip input");
        let bytes = encoder.finish().expect("finish gzip");
        let mut file = tempfile::tempfile().expect("temporary gzip");
        file.write_all(&bytes).expect("write compressed data");
        file.rewind().expect("rewind compressed data");
        file
    }

    #[test]
    fn gzip_decoder_rejects_bytes_after_the_last_member() {
        let mut file = gzip(b"valid tar bytes");
        file.write_all(b"unparsed trailing bytes")
            .expect("append trailing bytes");
        file.rewind().expect("rewind compressed data");
        assert!(decode_archive(file, ArchiveCompression::Gzip, None).is_err());
    }

    #[test]
    fn gzip_decoder_returns_the_complete_decompressed_stream() {
        let file = gzip(b"valid tar bytes");
        let mut decoded =
            decode_archive(file, ArchiveCompression::Gzip, None).expect("decode complete gzip");
        let mut bytes = Vec::new();
        decoded
            .read_to_end(&mut bytes)
            .expect("read decompressed data");
        assert_eq!(bytes, b"valid tar bytes");
    }
}
