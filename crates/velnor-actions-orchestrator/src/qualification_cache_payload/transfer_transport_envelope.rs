//! Length-delimited v1 transport around one raw POSIX tar stream.

use std::io::{Read, Seek, SeekFrom, Write};

const MAGIC: &[u8] = b"velnor-cache-transport-v1\n";
const LENGTH_BYTES: usize = 8;
const LENGTH_BYTES_U64: u64 = 8;
const MIN_TAR_BYTES: u64 = 1_024;
const TAR_BLOCK_BYTES: u64 = 512;
const MAX_TRANSPORT_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const HEADER_BYTES: u64 = 34;
const _: () = assert!(MAGIC.len() == 26);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::qualification_cache_payload) struct RawTarRange {
    pub(in crate::qualification_cache_payload) offset: u64,
    pub(in crate::qualification_cache_payload) length: u64,
}

pub(super) fn write_envelope<W, F>(
    writer: &mut W,
    operation_limit: u64,
    write_tar: F,
) -> Result<(), &'static str>
where
    W: Write + Seek,
    F: FnOnce(&mut LimitedWriter<'_, W>) -> Result<(), &'static str>,
{
    validate_operation_limit(operation_limit)?;
    writer
        .seek(SeekFrom::Start(0))
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    writer
        .write_all(MAGIC)
        .and_then(|()| writer.write_all(&[0; LENGTH_BYTES]))
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    let tar_offset = writer
        .stream_position()
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    let tar_limit = operation_limit
        .checked_sub(HEADER_BYTES)
        .ok_or("qualification_cache_transport_limit_invalid")?;
    let mut limited = LimitedWriter {
        writer,
        remaining: tar_limit,
        exceeded: false,
    };
    let result = write_tar(&mut limited);
    if limited.exceeded {
        return Err("qualification_cache_transport_limit_invalid");
    }
    result?;
    let writer = limited.writer;
    writer
        .flush()
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    let end = writer
        .stream_position()
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    let length = end
        .checked_sub(tar_offset)
        .filter(|length| valid_tar_length(*length, operation_limit))
        .ok_or("qualification_cache_transport_length_invalid")?;
    let length_offset = tar_offset
        .checked_sub(LENGTH_BYTES_U64)
        .ok_or("qualification_cache_transport_write_failed")?;
    writer
        .seek(SeekFrom::Start(length_offset))
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    writer
        .write_all(&length.to_be_bytes())
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    writer
        .seek(SeekFrom::Start(end))
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    Ok(())
}

pub(super) struct LimitedWriter<'a, W> {
    writer: &'a mut W,
    remaining: u64,
    exceeded: bool,
}

impl<W: Write> Write for LimitedWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            self.exceeded = true;
            return Err(std::io::Error::other("cache transport operation limit"));
        }
        let allowed = usize::try_from(self.remaining)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let written = self.writer.write(&bytes[..allowed])?;
        self.remaining = self
            .remaining
            .saturating_sub(u64::try_from(written).unwrap_or(u64::MAX));
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

pub(super) fn read_envelope<R: Read + Seek>(
    reader: &mut R,
    operation_limit: u64,
) -> Result<RawTarRange, &'static str> {
    validate_operation_limit(operation_limit)?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| "qualification_cache_transport_read_failed")?;
    let mut magic = [0; MAGIC.len()];
    let mut encoded_length = [0; LENGTH_BYTES];
    reader
        .read_exact(&mut magic)
        .and_then(|()| reader.read_exact(&mut encoded_length))
        .map_err(|_| "qualification_cache_transport_invalid")?;
    if magic.as_slice() != MAGIC {
        return Err("qualification_cache_transport_invalid");
    }
    let length = u64::from_be_bytes(encoded_length);
    if !valid_tar_length(length, operation_limit) {
        return Err("qualification_cache_transport_length_invalid");
    }
    let offset = HEADER_BYTES;
    let expected_end = offset
        .checked_add(length)
        .ok_or("qualification_cache_transport_length_invalid")?;
    let actual_end = reader
        .seek(SeekFrom::End(0))
        .map_err(|_| "qualification_cache_transport_read_failed")?;
    if expected_end != actual_end {
        return Err("qualification_cache_transport_trailing_or_truncated");
    }
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|_| "qualification_cache_transport_read_failed")?;
    {
        let mut raw_tar = reader.take(length);
        velnor_archive_guard::owned_archive_guard::validate(&mut raw_tar, "cache-payload")
            .map_err(|_| "qualification_cache_transport_tar_invalid")?;
    }
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|_| "qualification_cache_transport_read_failed")?;
    Ok(RawTarRange { offset, length })
}

fn valid_tar_length(length: u64, operation_limit: u64) -> bool {
    (MIN_TAR_BYTES..=operation_limit - HEADER_BYTES).contains(&length)
        && length.is_multiple_of(TAR_BLOCK_BYTES)
}

fn validate_operation_limit(limit: u64) -> Result<(), &'static str> {
    if !(HEADER_BYTES + MIN_TAR_BYTES..=MAX_TRANSPORT_BYTES).contains(&limit) {
        return Err("qualification_cache_transport_limit_invalid");
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/dispatch_cache_transport_envelope_tests.rs"]
mod tests;
