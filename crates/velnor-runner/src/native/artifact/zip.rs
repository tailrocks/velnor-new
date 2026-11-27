// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use ring::digest::{Context as Sha256, SHA256};
use rustix::fs::OFlags;
use uuid::Uuid;

use super::mtime::dos_time;
use zip::DateTime;
use zip::read::Config;
use zip::unstable::read::ZipIterable;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub(super) const MAX_DOWNLOAD_BYTES: u64 = 5 << 30;
const MAX_UPLOAD_BYTES: u64 = (1 << 32) - 1;
const MAX_MEMBERS: usize = 100_000;
const MAX_EXTRACTED_BYTES: u64 = 5 << 30;
const COMPRESSION_LEVEL: i64 = 6;
const BUFFER_BYTES: usize = 64 * 1024;

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[derive(Debug)]
pub(super) struct TempZip(PathBuf);

impl TempZip {
    pub(super) fn new(directory: &Path) -> Self {
        Self(directory.join(format!("velnor-artifact-{}.zip", Uuid::new_v4())))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempZip {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Debug)]
pub(super) struct Written {
    pub(super) size: u64,
    pub(super) sha256: String,
}

fn cancelled(stop: &AtomicBool) -> Result<()> {
    if stop.load(Ordering::Relaxed) {
        bail!("the step was cancelled");
    }
    Ok(())
}

fn copy(reader: &mut impl Read, writer: &mut impl Write, stop: &AtomicBool) -> Result<u64> {
    let mut buffer = vec![0_u8; BUFFER_BYTES];
    let mut total = 0_u64;
    loop {
        cancelled(stop)?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(total);
        }
        writer.write_all(&buffer[..read])?;
        total += u64::try_from(read).unwrap_or(u64::MAX);
    }
}

fn hash_file(file: &mut File, stop: &AtomicBool) -> Result<String> {
    let mut hash = Sha256::new(&SHA256);
    let mut buffer = vec![0_u8; BUFFER_BYTES];
    loop {
        cancelled(stop)?;
        let read = file.read(&mut buffer).context("read the zip")?;
        if read == 0 {
            return Ok(hex(hash.finish().as_ref()));
        }
        hash.update(&buffer[..read]);
    }
}

const UPLOAD_LIMIT_ENV: &str = "VELNOR_ARTIFACT_UPLOAD_LIMIT";
const FILES_LIMIT_ENV: &str = "VELNOR_ARTIFACT_FILES_LIMIT";

fn limit_of(variable: &str, default: u64) -> u64 {
    if cfg!(feature = "test-support") {
        std::env::var(variable)
            .ok()
            .and_then(|text| text.parse::<u64>().ok())
            .unwrap_or(default)
    } else {
        default
    }
}

struct Capped {
    file: File,
    position: u64,
    size: u64,
    limit: u64,
    exceeded: Arc<AtomicBool>,
}

impl Write for Capped {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.file.write(buffer)?;
        self.position += u64::try_from(written).unwrap_or(u64::MAX);
        self.size = self.size.max(self.position);
        if self.size > self.limit {
            self.exceeded.store(true, Ordering::Relaxed);
            return Err(std::io::Error::other(
                "the archive is over the upload limit",
            ));
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Seek for Capped {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.position = self.file.seek(to)?;
        Ok(self.position)
    }
}

pub(super) fn write(
    destination: &Path,
    files: &[(PathBuf, String)],
    stop: &AtomicBool,
) -> Result<Written> {
    if files.len() > MAX_MEMBERS {
        bail!(
            "artifact has {} files, exceeding the {MAX_MEMBERS}-file limit; split it into artifacts with fewer files",
            files.len()
        );
    }
    let mut announced = 0_u64;
    for (source, _) in files {
        let metadata =
            fs::metadata(source).with_context(|| format!("stat {}", source.display()))?;
        announced = announced.saturating_add(metadata.len());
    }
    let files_limit = limit_of(FILES_LIMIT_ENV, MAX_EXTRACTED_BYTES);
    if announced > files_limit {
        bail!(
            "artifact holds more than {files_limit} bytes of files, which download-artifact refuses to extract; split it into artifacts of at most that size"
        );
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)
        .with_context(|| format!("create {}", destination.display()))?;
    let limit = limit_of(UPLOAD_LIMIT_ENV, MAX_UPLOAD_BYTES);
    let exceeded = Arc::new(AtomicBool::new(false));
    let output = Capped {
        file,
        position: 0,
        size: 0,
        limit,
        exceeded: Arc::clone(&exceeded),
    };
    let written = build(output, files, files_limit, stop);
    if exceeded.load(Ordering::Relaxed) {
        bail!(
            "artifact is larger than the {limit}-byte upload limit; split it into smaller uploads"
        );
    }
    written
}

fn build(
    output: Capped,
    files: &[(PathBuf, String)],
    files_limit: u64,
    stop: &AtomicBool,
) -> Result<Written> {
    let mut zip = ZipWriter::new(output);
    let mut remaining = files_limit;
    for (source, entry) in files {
        let mut input = File::open(source).with_context(|| format!("open {}", source.display()))?;
        let metadata = input
            .metadata()
            .with_context(|| format!("stat {}", source.display()))?;
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(COMPRESSION_LEVEL))
            .unix_permissions(metadata.permissions().mode() & 0o777)
            .last_modified_time(
                metadata
                    .modified()
                    .map_or_else(|_| DateTime::default(), dos_time),
            )
            .large_file(remaining.saturating_add(1) >= u64::from(u32::MAX));
        zip.start_file(entry, options)
            .with_context(|| format!("add {entry} to the zip"))?;
        let copied = copy(
            &mut (&mut input).take(remaining.saturating_add(1)),
            &mut zip,
            stop,
        )
        .with_context(|| format!("read {}", source.display()))?;
        if copied > remaining {
            bail!(
                "a file grew while it was being archived and the artifact now holds more than {files_limit} bytes of files, which download-artifact refuses to extract; archive the files once they stop changing"
            );
        }
        remaining -= copied;
    }
    let mut output = zip.finish().context("finish the zip")?.file;
    output.flush().context("flush the zip")?;
    let size = output.metadata().context("stat the zip")?.len();
    std::io::Seek::rewind(&mut output).context("rewind the zip")?;
    Ok(Written {
        size,
        sha256: hash_file(&mut output, stop)?,
    })
}

fn ensure_dir(base: &Path, relative: &Path) -> Result<PathBuf> {
    let mut current = base.to_path_buf();
    for part in relative.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let real = fs::canonicalize(&current)
                    .with_context(|| format!("resolve {}", current.display()))?;
                if !real.starts_with(base) {
                    bail!(
                        "artifact entry '{}' leads through a symbolic link outside the destination directory",
                        relative.display()
                    );
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .with_context(|| format!("create {}", current.display()))?;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("stat {}", current.display()));
            }
        }
    }
    Ok(current)
}

fn create_fresh(target: &Path) -> Result<File> {
    if fs::symlink_metadata(target).is_ok_and(|metadata| metadata.file_type().is_file()) {
        return OpenOptions::new()
            .write(true)
            .truncate(true)
            .custom_flags(OFlags::NOFOLLOW.bits().cast_signed())
            .open(target)
            .with_context(|| format!("replace {}", target.display()));
    }
    match fs::remove_file(target) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("replace {}", target.display()));
        }
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o666)
        .open(target)
        .with_context(|| format!("create {}", target.display()))
}

fn check_member_count(file: &File) -> Result<()> {
    let records =
        ZipIterable::try_new(file, Config::default()).context("read the downloaded zip")?;
    if records.len() > MAX_MEMBERS {
        bail!("artifact contains more than {MAX_MEMBERS} files");
    }
    Ok(())
}

pub(super) fn install(
    archive: &Path,
    destination: &Path,
    raw_name: Option<&str>,
    stop: &AtomicBool,
) -> Result<()> {
    match raw_name {
        Some(name) => store(archive, destination, name, stop),
        None => extract(archive, destination, stop),
    }
}

fn store(archive: &Path, destination: &Path, name: &str, stop: &AtomicBool) -> Result<()> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    let base = fs::canonicalize(destination)
        .with_context(|| format!("resolve {}", destination.display()))?;
    let target = base.join(name);
    let mut input = File::open(archive).context("open the downloaded file")?;
    let mut output = create_fresh(&target)?;
    copy(&mut input, &mut output, stop).with_context(|| format!("write {}", target.display()))?;
    Ok(())
}

fn extract(archive: &Path, destination: &Path, stop: &AtomicBool) -> Result<()> {
    let file = File::open(archive).context("open the downloaded zip")?;
    check_member_count(&file)?;
    let mut zip = ZipArchive::new(file).context("read the downloaded zip")?;
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let member = zip.by_index(index).context("read a zip entry")?;
        if member.enclosed_name().is_none() {
            bail!(
                "artifact entry '{}' is outside the destination directory",
                String::from_utf8_lossy(member.name_raw())
            );
        }
        if member.is_symlink() {
            bail!(
                "artifact entry '{}' is a symbolic link",
                String::from_utf8_lossy(member.name_raw())
            );
        }
        total = total.saturating_add(member.size());
        if total > MAX_EXTRACTED_BYTES {
            bail!("artifact is larger than the {MAX_EXTRACTED_BYTES}-byte extraction limit");
        }
    }
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    let base = fs::canonicalize(destination)
        .with_context(|| format!("resolve {}", destination.display()))?;
    let mut remaining = MAX_EXTRACTED_BYTES;
    for index in 0..zip.len() {
        let mut member = zip.by_index(index).context("read a zip entry")?;
        let Some(relative) = member.enclosed_name() else {
            bail!(
                "artifact entry '{}' is outside the destination directory",
                String::from_utf8_lossy(member.name_raw())
            );
        };
        if member.is_dir() {
            ensure_dir(&base, &relative)?;
            continue;
        }
        let (Some(name), Some(parent)) = (relative.file_name(), relative.parent()) else {
            continue;
        };
        let target = ensure_dir(&base, parent)?.join(name);
        let mut output = create_fresh(&target)?;
        let copied = copy(&mut (&mut member).take(remaining + 1), &mut output, stop)
            .with_context(|| format!("extract {}", target.display()))?;
        if copied > remaining {
            bail!("artifact is larger than the {MAX_EXTRACTED_BYTES}-byte extraction limit");
        }
        remaining -= copied;
    }
    Ok(())
}
