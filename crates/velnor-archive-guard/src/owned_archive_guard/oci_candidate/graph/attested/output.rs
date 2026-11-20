// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
};

use crate::owned_archive_guard::oci_candidate::{
    OciCandidateError, Result, archive::OciArchive, check_deadline,
};

pub(super) fn write(
    archive: &mut OciArchive,
    destination: &mut File,
    index: &[u8],
    references: &BTreeMap<String, (u64, String)>,
) -> Result<()> {
    check_deadline()?;
    destination
        .seek(SeekFrom::Start(0))
        .map_err(|_| OciCandidateError("projection_seek"))?;
    let mut output = tar::Builder::new(&mut *destination);
    append(&mut output, "oci-layout", archive.layout.as_slice())?;
    append(&mut output, "index.json", index)?;
    for digest in references.keys() {
        check_deadline()?;
        let blob = archive
            .blobs
            .get(digest)
            .ok_or(OciCandidateError("oci_blob_missing"))?;
        archive
            .file
            .seek(SeekFrom::Start(blob.offset))
            .map_err(|_| OciCandidateError("projection_seek"))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(blob.size);
        header.set_mode(0o600);
        header.set_cksum();
        let reader = CheckedReader {
            inner: (&mut archive.file).take(blob.size),
        };
        output
            .append_data(&mut header, format!("blobs/sha256/{digest}"), reader)
            .map_err(|_| OciCandidateError("projection_write"))?;
    }
    output
        .finish()
        .map_err(|_| OciCandidateError("projection_write"))?;
    drop(output);
    destination
        .sync_all()
        .map_err(|_| OciCandidateError("projection_sync"))?;
    check_deadline()
}

fn append(output: &mut tar::Builder<&mut File>, name: &str, bytes: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(u64::try_from(bytes.len()).map_err(|_| OciCandidateError("projection_size"))?);
    header.set_mode(0o600);
    header.set_cksum();
    output
        .append_data(&mut header, name, bytes)
        .map_err(|_| OciCandidateError("projection_write"))
}

struct CheckedReader<R> {
    inner: R,
}

impl<R: Read> Read for CheckedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        check_deadline().map_err(|_| io::Error::other("projection deadline"))?;
        self.inner.read(buffer)
    }
}
