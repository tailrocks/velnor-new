// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeSet, BufReader, File, GzDecoder, Instant, MAX_ARCHIVE, MAX_ENTRIES, MAX_FILE, MAX_TOTAL,
    OrchestratorError, Read, Seek, SeekFrom, invalid, io,
};
pub(super) fn preflight(input: &mut File, deadline: Instant) -> Result<(), OrchestratorError> {
    input.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
    let mut zip = zip::ZipArchive::new(input).map_err(|_| invalid())?;
    if zip.len() != 1 {
        return Err(invalid());
    }
    let entry = zip.by_index(0).map_err(|_| invalid())?;
    if entry.name_raw() != b"rust-toolchain.tar.gz"
        || entry.is_dir()
        || entry.is_symlink()
        || entry.encrypted()
        || entry.unix_mode().is_some_and(|mode| {
            let kind = mode & 0o170_000;
            (kind != 0 && kind != 0o100_000) || mode & 0o7022 != 0
        })
        || entry.size() == 0
        || entry.size() > MAX_ARCHIVE
    {
        return Err(invalid());
    }
    let mut decoder = GzDecoder::new(BufReader::new(Timed {
        inner: entry,
        deadline,
    }));
    let mut archive = tar::Archive::new(&mut decoder);
    let mut total = 0u64;
    let mut count = 0usize;
    for item in archive.entries().map_err(|_| invalid())?.raw(true) {
        if Instant::now() >= deadline {
            return Err(invalid());
        }
        count += 1;
        if count > MAX_ENTRIES {
            return Err(invalid());
        }
        let mut item = item.map_err(|_| invalid())?;
        let kind = item.header().entry_type();
        let size = item.size();
        total = total
            .checked_add(size)
            .filter(|n| *n <= MAX_TOTAL)
            .ok_or_else(invalid)?;
        if kind.is_gnu_longname() || kind.is_pax_local_extensions() {
            if size > 8192 {
                return Err(invalid());
            }
            let mut data = Vec::new();
            item.read_to_end(&mut data).map_err(|_| invalid())?;
            if kind.is_pax_local_extensions() {
                validate_pax(&data)?;
            }
        } else if !(kind.is_file() || kind.is_dir()) || size > MAX_FILE {
            return Err(invalid());
        }
    }
    let _inner = archive.into_inner();
    if count == 0 {
        return Err(invalid());
    }
    finish_gzip(decoder, deadline)?;
    Ok(())
}
fn validate_pax(mut data: &[u8]) -> Result<(), OrchestratorError> {
    let mut keys = BTreeSet::new();
    while !data.is_empty() {
        let space = data.iter().position(|b| *b == b' ').ok_or_else(invalid)?;
        let len: usize = std::str::from_utf8(&data[..space])
            .map_err(|_| invalid())?
            .parse()
            .map_err(|_| invalid())?;
        if len <= space + 2 || len > data.len() || data[len - 1] != b'\n' {
            return Err(invalid());
        }
        let record = &data[space + 1..len - 1];
        let sep = record.iter().position(|b| *b == b'=').ok_or_else(invalid)?;
        if !keys.insert(record[..sep].to_vec()) {
            return Err(invalid());
        }
        if !matches!(
            &record[..sep],
            b"path" | b"mtime" | b"atime" | b"ctime" | b"uid" | b"gid" | b"uname" | b"gname"
        ) {
            return Err(invalid());
        }
        data = &data[len..];
    }
    Ok(())
}

pub(super) struct Timed<R> {
    pub(super) inner: R,
    pub(super) deadline: Instant,
}
impl<R: Read> Read for Timed<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "deadline"));
        }
        self.inner.read(buf)
    }
}

pub(super) fn finish_gzip<R: Read>(
    mut decoder: GzDecoder<BufReader<R>>,
    deadline: Instant,
) -> Result<(), OrchestratorError> {
    let mut tail = 0usize;
    let mut buf = [0u8; 16384];
    loop {
        let n = decoder.read(&mut buf).map_err(|_| invalid())?;
        if n == 0 {
            break;
        }
        if Instant::now() >= deadline || buf[..n].iter().any(|b| *b != 0) {
            return Err(invalid());
        }
        tail = tail
            .checked_add(n)
            .filter(|n| *n <= 1024 * 1024)
            .ok_or_else(invalid)?;
    }
    if tail < 512 {
        return Err(invalid());
    }
    let mut source = decoder.into_inner();
    if source.read(&mut [0u8; 1]).map_err(|_| invalid())? != 0 {
        return Err(invalid());
    }
    Ok(())
}
