// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use velnor_model::RepoPath;
use velnor_rust::{MetadataError, cargo_files};

use crate::expected_files;

use super::{Error, read_view, regular_file, replace, toplevel};

pub const SAMPLES: &str = "generator/samples";
pub const SAMPLE_REPOSITORY: &str = "/repo";

pub fn record_sample(destination: &Path, checkout: &Path, name: &str) -> Result<(), Error> {
    let name = sample_name(name)?;
    let destination = toplevel(destination)?;
    let checkout = toplevel(checkout)?;
    let (view, metadata) = read_view(&checkout, true)?;
    expected_files(&view).map_err(Error::Generate)?;
    let recording = Recording {
        view: view.files,
        manifests: cargo_files(&view.tracked_paths),
        metadata: metadata
            .into_iter()
            .map(|(workspace, json)| Ok((workspace, relocate_metadata(&checkout, &json)?)))
            .collect::<Result<_, Error>>()?,
    };
    write_sample(&destination, name, &recording)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    pub view: BTreeMap<RepoPath, Vec<u8>>,
    pub manifests: Vec<RepoPath>,
    pub metadata: BTreeMap<RepoPath, Vec<u8>>,
}

pub fn relocate_metadata(checkout: &Path, json: &[u8]) -> Result<Vec<u8>, Error> {
    let from = checkout
        .to_str()
        .ok_or_else(|| Error::Repository("the checkout path is not UTF-8".to_owned()))?
        .trim_end_matches('/');
    if from.is_empty() {
        return Err(Error::Repository(
            "a checkout at the filesystem root cannot be recorded".to_owned(),
        ));
    }
    let mut value: serde_json::Value = serde_json::from_slice(json)
        .map_err(|e| Error::Metadata(MetadataError::Invalid(e.to_string())))?;
    let encoded = percent_encode(from);
    relocate(&mut value, &[from, &encoded]);
    let mut bytes = serde_json::to_vec_pretty(&value)
        .map_err(|e| Error::Metadata(MetadataError::Invalid(e.to_string())))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn relocate_text(text: &str, from: &[&str]) -> String {
    from.iter().fold(text.to_owned(), |text, prefix| {
        text.replace(prefix, SAMPLE_REPOSITORY)
    })
}

fn relocate(value: &mut serde_json::Value, from: &[&str]) {
    match value {
        serde_json::Value::String(text) => *text = relocate_text(text, from),
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| relocate(v, from)),
        serde_json::Value::Object(map) => {
            let entries = std::mem::take(map);
            for (key, mut entry) in entries {
                relocate(&mut entry, from);
                map.insert(relocate_text(&key, from), entry);
            }
        }
        _ => {}
    }
}

pub fn write_sample(root: &Path, name: &str, recording: &Recording) -> Result<(), Error> {
    let base = format!("{SAMPLES}/{}", sample_name(name)?);
    let mut files = Vec::new();
    for (path, bytes) in &recording.view {
        files.push((sample_path(format!("{base}/view/{path}"))?, bytes.clone()));
    }
    for (path, bytes) in &recording.metadata {
        files.push((
            sample_path(format!("{base}/cargo-metadata/{path}.json"))?,
            bytes.clone(),
        ));
    }
    let mut listing = String::new();
    for manifest in &recording.manifests {
        listing.push_str(manifest.as_str());
        listing.push('\n');
    }
    files.push((
        sample_path(format!("{base}/manifests.txt"))?,
        listing.into_bytes(),
    ));
    let directories = [
        sample_path(format!("{base}/view"))?,
        sample_path(format!("{base}/cargo-metadata"))?,
    ];
    for (path, _) in &files {
        regular_file(root, path)?;
    }
    let existing = directories
        .iter()
        .map(|directory| real_directory(root, directory))
        .collect::<Result<Vec<_>, _>>()?;
    for directory in existing.into_iter().flatten() {
        fs::remove_dir_all(&directory).map_err(|e| Error::File(directory, e))?;
    }
    for (path, bytes) in files {
        replace(&root.join(path.as_str()), &bytes)?;
    }
    Ok(())
}

fn sample_name(name: &str) -> Result<&str, Error> {
    let valid = !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(name)
    } else {
        Err(Error::Repository(format!(
            "sample name {name:?} must hold lowercase letters, digits and dashes"
        )))
    }
}

fn sample_path(text: String) -> Result<RepoPath, Error> {
    RepoPath::new(text).map_err(|e| Error::Repository(e.to_string()))
}

fn real_directory(root: &Path, path: &RepoPath) -> Result<Option<PathBuf>, Error> {
    let mut current = root.to_path_buf();
    for part in path.as_str().split('/') {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(Error::NotRegular(path.clone())),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(Error::File(current, error)),
        }
    }
    Ok(Some(current))
}
