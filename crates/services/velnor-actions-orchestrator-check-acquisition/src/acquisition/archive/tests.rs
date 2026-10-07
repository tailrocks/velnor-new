use super::super::*;
use super::*;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tar::{EntryType, Header};
use zip::write::{SimpleFileOptions, ZipWriter};

fn tar_gz_extension_with_declared_size(path: &Path, kind: EntryType, size: u64) {
    let file = fs::File::create(path).expect("archive");
    let mut encoder = GzEncoder::new(file, Compression::default());
    let mut header = Header::new_gnu();
    header.set_path("metadata").expect("path");
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_cksum();
    encoder.write_all(header.as_bytes()).expect("header");
    encoder
        .write_all(b"tiny")
        .expect("truncated metadata payload");
    encoder.finish().expect("gzip finish");
}
fn scratch(name: &str) -> tempfile::TempDir {
    tempfile::TempDir::with_prefix(format!("tool-archive-{name}-")).expect("temp")
}
fn extract(archive: &Path, destination: &Path, url: &str) -> Result<(), OrchestratorError> {
    let mut budget = ArchiveBudget::new();
    let deadline = CheckDeadline::after(Duration::from_secs(60)).expect("deadline");
    extract_archive(archive, destination, url, &mut budget, deadline)
}
fn scratch_root(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().canonicalize().expect("temp root")
}

fn zip_file(path: &Path, entries: &[(&str, &[u8], u32)]) {
    let file = fs::File::create(path).expect("archive");
    let mut writer = ZipWriter::new(file);
    for (name, data, mode) in entries {
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(*mode);
        writer.start_file(*name, options).expect("file");
        writer.write_all(data).expect("data");
    }
    writer.finish().expect("zip finish");
}

mod archive_tests;
mod deadline_tests;
mod metadata_tests;
mod tar_admission_tests;
mod zip_admission_tests;
