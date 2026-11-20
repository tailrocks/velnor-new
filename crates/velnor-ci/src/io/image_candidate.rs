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
use crate::image_candidate::oci::{self, BlobSource};
use crate::image_candidate::record::{INPUT_PATHS, validate};

const MAX_RECORD_BYTES: u64 = 1 << 20;
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
    for name in INPUT_PATHS {
        let data = fs::read(root.join(name))
            .map_err(|error| CiError::new(format!("{}: {error}", root.join(name).display())))?;
        inputs.insert(name.to_owned(), Value::String(sha256_hex(&data)));
    }
    Ok(Value::Object(inputs))
}

fn locked_buildx(root: &Path) -> Result<String, CiError> {
    let path = root.join(".velnor/runner.lock");
    let text = fs::read_to_string(&path)
        .map_err(|error| CiError::new(format!("{}: {error}", path.display())))?;
    let lock: toml::Table = toml::from_str(&text)
        .map_err(|error| CiError::new(format!("{}: {error}", path.display())))?;
    lock.get("buildx_version")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| CiError::new(format!("{}: buildx_version is missing", path.display())))
}

pub(super) fn verify_candidate(args: &VerifyCandidateArgs) -> Result<String, CiError> {
    let env: HashMap<String, String> = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .filter(|(name, _)| name.starts_with("GITHUB_"))
        .collect();
    verify_with(args, &env)
}

fn verify_with(
    args: &VerifyCandidateArgs,
    env: &HashMap<String, String>,
) -> Result<String, CiError> {
    let raw = read_record(&args.record)?;
    let value = parse(&raw, "candidate record")?;
    let record = validate(&value)?;
    let scan = scan_archive(&args.archive)?;
    let actual = oci::verify(
        &scan,
        &FileSource {
            path: args.archive.clone(),
        },
    )?;
    if record["archive"] != json!({"sha256": scan.archive_sha, "size_bytes": scan.archive_size}) {
        return fail("Candidate archive is not the same as the archive in its record.");
    }
    if record["oci"] != actual {
        return fail("OCI graph is not the same as the graph in its candidate record.");
    }
    let root = fs::canonicalize(&args.root)
        .map_err(|error| CiError::new(format!("{}: {error}", args.root.display())))?;
    if record["source"]["inputs"] != source_inputs(&root)? {
        return fail("Build input bytes changed after candidate creation.");
    }
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
    let locked = locked_buildx(&root)?;
    let builder = record["build"]["builder"].as_str().unwrap_or_default();
    if buildx_version(builder) != Some(locked.as_str()) {
        return fail(
            "Candidate Buildx identity is not the same as the value in .velnor/runner.lock.",
        );
    }
    Ok(PASS.to_owned())
}
