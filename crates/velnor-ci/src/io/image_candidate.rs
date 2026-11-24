// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::cli::VerifyCandidateArgs;
use crate::error::CiError;
use crate::image_candidate::archive::{self, Blob, MAX_ARCHIVE_BYTES};
use crate::image_candidate::hashing::sha256_hex;
use crate::image_candidate::identity::{buildx_version, workflow};
use crate::image_candidate::json::parse;
use crate::image_candidate::lock::{Lock, parse_lock};
use crate::image_candidate::oci::{self, BlobSource};
use crate::image_candidate::record::{INPUT_PATHS, validate};

mod build;
mod client;
mod docker;
mod output;
mod process;
pub(in crate::io) mod publish;
mod source;

pub(in crate::io) use build::build;

const DOCKERFILE_DIR: &str = "images/job/ubuntu-26.04";
const DOCKERFILE: &str = "images/job/ubuntu-26.04/Dockerfile";
const MAX_RECORD_BYTES: u64 = 1 << 20;
const MAX_INPUT_BYTES: u64 = 1 << 24;
const PASS: &str =
    "PASS: Candidate record has the same source inputs and OCI archive identities.\n";

struct FileSource {
    path: PathBuf,
}

impl BlobSource for FileSource {
    fn open(&self, blob: &Blob) -> io::Result<Box<dyn Read + Send + '_>> {
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(blob.offset))?;
        Ok(Box::new(BufReader::with_capacity(
            1 << 20,
            file.take(blob.size),
        )))
    }
}

fn fail<T>(message: impl Into<String>) -> Result<T, CiError> {
    Err(CiError::new(message))
}

fn regular(path: &Path, label: &str) -> Result<u64, CiError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(meta.len()),
        _ => fail(format!("{label} must be a regular file.")),
    }
}

fn read_record(path: &Path) -> Result<Vec<u8>, CiError> {
    if regular(path, "Candidate record")? > MAX_RECORD_BYTES {
        return fail("Candidate record exceeds its size limit.");
    }
    let mut data = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut data))
        .map_err(|error| CiError::new(format!("{}: {error}", path.display())))?;
    if data.len() as u64 > MAX_RECORD_BYTES {
        return fail("Candidate record exceeds its size limit.");
    }
    Ok(data)
}

fn scan_archive(path: &Path) -> Result<archive::Scanned, CiError> {
    if regular(path, "OCI archive")? > MAX_ARCHIVE_BYTES {
        return fail("OCI archive exceeds its file size limit.");
    }
    let unreadable = |_: io::Error| CiError::new("OCI archive extension scan failed.");
    let mut file = File::open(path).map_err(unreadable)?;
    let mut prefix = [0u8; 6];
    let count = file.read(&mut prefix).map_err(unreadable)?;
    let prefix = &prefix[..count];
    if prefix.starts_with(&[0x1f, 0x8b])
        || prefix.starts_with(b"BZh")
        || prefix.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0])
    {
        return fail("OCI archive must use an uncompressed tar format.");
    }
    file.seek(SeekFrom::Start(0)).map_err(unreadable)?;
    archive::scan(BufReader::with_capacity(1 << 20, file))
}

fn source_inputs(root: &Path) -> Result<Value, CiError> {
    let mut inputs = Map::new();
    for name in INPUT_PATHS.iter().copied() {
        let path = root.join(name);
        if regular(&path, name)? > MAX_INPUT_BYTES {
            return fail(format!("{name} exceeds its size limit."));
        }
        let data = fs::read(&path)
            .map_err(|error| CiError::new(format!("{}: {error}", path.display())))?;
        inputs.insert(name.to_owned(), Value::String(sha256_hex(&data)));
    }
    Ok(Value::Object(inputs))
}

fn read_lock(root: &Path) -> Result<Lock, CiError> {
    let path = root.join(".velnor/runner.lock");
    let text = fs::read_to_string(&path)
        .map_err(|error| CiError::new(format!("{}: {error}", path.display())))?;
    parse_lock(&text).map_err(|error| error.context(&path.display().to_string()))
}

fn github_env() -> HashMap<String, String> {
    std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .filter(|(name, _)| name.starts_with("GITHUB_"))
        .collect()
}

pub(super) fn verify_candidate(args: &VerifyCandidateArgs) -> Result<String, CiError> {
    verify_with(args, &github_env())
}

struct Checked {
    raw: Vec<u8>,
    record: Value,
    scan: archive::Scanned,
}

fn check_archive(record: &Path, archive: &Path, root: &Path) -> Result<Checked, CiError> {
    let raw = read_record(record)?;
    let value = parse(&raw, "candidate record")?;
    validate(&value)?;
    let scan = scan_archive(archive)?;
    let actual = oci::verify(
        &scan,
        &FileSource {
            path: archive.to_path_buf(),
        },
        value["source"]["commit"].as_str().unwrap_or_default(),
    )?;
    if value["archive"] != json!({"sha256": scan.archive_sha, "size_bytes": scan.archive_size}) {
        return fail("Candidate archive is not the same as the archive in its record.");
    }
    if value["oci"] != actual {
        return fail("OCI graph is not the same as the graph in its candidate record.");
    }
    let root = fs::canonicalize(root)
        .map_err(|error| CiError::new(format!("{}: {error}", root.display())))?;
    if value["source"]["inputs"] != source_inputs(&root)? {
        return fail("Build input bytes changed after candidate creation.");
    }
    Ok(Checked {
        raw,
        record: value,
        scan,
    })
}

fn check_builder(root: &Path, record: &Value) -> Result<(), CiError> {
    let lock = read_lock(root)?;
    let builder = record["build"]["builder"].as_str().unwrap_or_default();
    if buildx_version(builder) != Some(lock.buildx_version.as_str()) {
        return fail(
            "Candidate Buildx identity is not the same as the value in .velnor/runner.lock.",
        );
    }
    Ok(())
}

fn verify_with(
    args: &VerifyCandidateArgs,
    env: &HashMap<String, String>,
) -> Result<String, CiError> {
    let Checked { record, .. } = check_archive(&args.record, &args.archive, &args.root)?;
    let root = fs::canonicalize(&args.root)
        .map_err(|error| CiError::new(format!("{}: {error}", args.root.display())))?;
    let (identity, reference) = workflow(env)?;
    let commit = env.get("GITHUB_SHA");
    if commit.map(String::as_str) != record["source"]["commit"].as_str()
        || record["source"]["ref"] != reference
    {
        return fail(
            "Candidate source identity is not the same as the current GitHub source identity.",
        );
    }
    if record["workflow"] != Value::Object(identity) {
        return fail(
            "Candidate workflow identity is not the same as the current GitHub workflow identity.",
        );
    }
    check_builder(&root, &record)?;
    Ok(PASS.to_owned())
}
