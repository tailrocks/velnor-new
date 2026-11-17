// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use velnor_model::{RepoPath, RepoView};
use velnor_rust::{MetadataError, cargo_files, manifests, wanted_files};

use crate::expected_with_notes;

use super::{Error, read_view, regular_file, replace, toplevel};
use staged::StagedSample;

mod staged;

pub const SAMPLES: &str = "generator/samples";
pub const SAMPLE_REPOSITORY: &str = "/repo";

pub fn record_sample(
    destination: &Path,
    checkout: &Path,
    name: &str,
) -> Result<SampleWriteOutcome, Error> {
    let name = sample_name(name)?;
    let destination = toplevel(destination)?;
    let checkout = toplevel(checkout)?;
    let (view, metadata) = read_view(&checkout, true)?;
    let recording = recording_of(&checkout, view, metadata, &mut std::io::stderr())?;
    write_sample(&destination, name, &recording)
}

pub fn recording_of(
    checkout: &Path,
    view: RepoView,
    metadata: BTreeMap<RepoPath, Vec<u8>>,
    out: &mut dyn Write,
) -> Result<Recording, Error> {
    let (_, notes) = expected_with_notes(&view).map_err(Error::Generate)?;
    for note in notes {
        writeln!(out, "velnor-gen: {note}").ok();
    }
    Ok(Recording {
        view: recorded_files(view.files, &view.tracked_paths),
        manifests: cargo_files(&view.tracked_paths),
        metadata: metadata
            .into_iter()
            .map(|(workspace, json)| Ok((workspace, relocate_metadata(checkout, &json)?)))
            .collect::<Result<_, Error>>()?,
    })
}

#[must_use]
pub fn recorded_files(
    files: BTreeMap<RepoPath, Vec<u8>>,
    tracked: &BTreeSet<RepoPath>,
) -> BTreeMap<RepoPath, Vec<u8>> {
    let mut wanted = wanted_files(tracked);
    wanted.extend(manifests(tracked));
    files
        .into_iter()
        .filter(|(path, _)| wanted.contains(path))
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    pub view: BTreeMap<RepoPath, Vec<u8>>,
    pub manifests: Vec<RepoPath>,
    pub metadata: BTreeMap<RepoPath, Vec<u8>>,
}

pub fn relocate_metadata(checkout: &Path, json: &[u8]) -> Result<Vec<u8>, Error> {
    let checkout = checkout
        .to_str()
        .ok_or_else(|| Error::Repository("the checkout path is not UTF-8".to_owned()))?;
    let original = checkout;
    let unc = checkout
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"));
    let checkout = unc
        .as_deref()
        .unwrap_or_else(|| checkout.strip_prefix(r"\\?\").unwrap_or(checkout));
    let from = checkout.trim_end_matches(['/', '\\']);
    let is_drive_root = from.len() == 2
        && from.as_bytes()[0].is_ascii_alphabetic()
        && from.as_bytes()[1] == b':'
        && from.len() < checkout.len();
    if from.is_empty() || is_drive_root {
        return Err(Error::Repository(
            "a checkout at the filesystem root cannot be recorded".to_owned(),
        ));
    }
    let mut value: serde_json::Value = serde_json::from_slice(json)
        .map_err(|e| Error::Metadata(MetadataError::Invalid(e.to_string())))?;
    let normalized = from.replace('\\', "/");
    let encoded = percent_encode(from);
    let encoded_normalized = percent_encode(&normalized);
    let mut prefixes = vec![
        from.to_owned(),
        normalized.clone(),
        encoded,
        encoded_normalized,
        original.trim_end_matches(['/', '\\']).to_owned(),
    ];
    if normalized != from {
        let url_path = format!("/{normalized}");
        prefixes.push(url_path.clone());
        prefixes.push(percent_encode(&url_path));
    }
    if let Some(authority) = normalized
        .strip_prefix("//")
        .filter(|rest| !rest.is_empty())
    {
        prefixes.push(authority.to_owned());
        prefixes.push(percent_encode(authority));
    }
    relocate(
        &mut value,
        &prefixes.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
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
    let mut prefixes = from.to_vec();
    prefixes.sort_by_key(|prefix| std::cmp::Reverse(prefix.len()));
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let in_file_url = cursor >= "file://".len() && text[..cursor].ends_with("file://");
        let at_boundary = cursor == 0
            || in_file_url
            || text[..cursor]
                .char_indices()
                .next_back()
                .is_some_and(|(index, character)| starts_path(text, index, character));
        if at_boundary
            && let Some(prefix) = prefixes.iter().find(|prefix| {
                text[cursor..].strip_prefix(**prefix).is_some_and(|tail| {
                    tail.is_empty()
                        || tail.starts_with(['/', '\\'])
                        || (in_file_url && tail.starts_with(['#', '?']))
                })
            })
        {
            let tail_start = cursor + prefix.len();
            let tail_end = tail_end(text, tail_start);
            let tail = text[tail_start..tail_end].replace('\\', "/");
            output.push_str(SAMPLE_REPOSITORY);
            output.push_str(&tail);
            cursor = tail_end;
            continue;
        }
        let Some(character) = text[cursor..].chars().next() else {
            break;
        };
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

fn tail_end(text: &str, start: usize) -> usize {
    let mut index = start;
    while let Some(character) = text[index..].chars().next() {
        if ends_path(text, index, character) {
            return index;
        }
        index += character.len_utf8();
    }
    text.len()
}

fn starts_path(text: &str, index: usize, character: char) -> bool {
    ends_path(text, index, character) || character == '='
}

fn ends_path(text: &str, index: usize, character: char) -> bool {
    character.is_whitespace()
        || matches!(character, ';' | ',' | '|')
        || (character == ':' && colon_ends_path(text, index))
}

fn colon_ends_path(text: &str, index: usize) -> bool {
    let rest = &text[index + 1..];
    if rest.starts_with("//") {
        return false;
    }
    rest.starts_with(['/', '\\'])
        || (rest.starts_with(|character: char| character.is_ascii_alphabetic())
            && rest[1..].starts_with(':'))
}

fn relocate(value: &mut serde_json::Value, from: &[&str]) -> Result<(), Error> {
    match value {
        serde_json::Value::String(text) => *text = relocate_text(text, from),
        serde_json::Value::Array(items) => {
            for item in items {
                relocate(item, from)?;
            }
        }
        serde_json::Value::Object(map) => {
            let entries = std::mem::take(map);
            for (key, mut entry) in entries {
                relocate(&mut entry, from)?;
                let key = relocate_text(&key, from);
                if map.insert(key.clone(), entry).is_some() {
                    return Err(Error::Repository(format!(
                        "recorded metadata has more than one object key relocated to {key:?}"
                    )));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum SampleWriteOutcome {
    Recorded,
    RecordedWithCleanupFailure {
        staging_path: PathBuf,
        error: String,
    },
}

pub fn write_sample(
    root: &Path,
    name: &str,
    recording: &Recording,
) -> Result<SampleWriteOutcome, Error> {
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
    for directory in directories {
        real_directory(root, &directory)?;
    }
    let existing = real_directory(root, &sample_path(base.clone())?)?;
    let samples = root.join(SAMPLES);
    fs::create_dir_all(&samples).map_err(|e| Error::File(samples.clone(), e))?;
    let staged = StagedSample::new(&samples)?;
    let prepared = staged.path.join("new");
    fs::create_dir(&prepared).map_err(|e| Error::File(prepared.clone(), e))?;
    if let Some(existing) = &existing {
        for entry in fs::read_dir(existing).map_err(|e| Error::File(existing.clone(), e))? {
            let entry = entry.map_err(|e| Error::File(existing.clone(), e))?;
            if !matches!(
                entry.file_name().to_str(),
                Some("view" | "cargo-metadata" | "manifests.txt")
            ) {
                copy_entry(&entry.path(), &prepared.join(entry.file_name()))?;
            }
        }
    }
    for (path, bytes) in files {
        let relative = sample_relative(&base, &path)?;
        replace(&prepared.join(relative), &bytes)?;
    }
    staged.publish(&root.join(base), existing.is_some())
}

fn sample_relative<'a>(base: &str, path: &'a RepoPath) -> Result<&'a str, Error> {
    path.as_str()
        .strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('/'))
        .ok_or_else(|| {
            Error::Repository(format!(
                "a recorded sample file must live in {base}: {}",
                path.as_str()
            ))
        })
}

fn copy_entry(source: &Path, destination: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(source).map_err(|e| Error::File(source.into(), e))?;
    let copied = if metadata.is_symlink() {
        copy_symlink(source, destination)
    } else if metadata.is_dir() {
        fs::create_dir(destination).map_err(|e| Error::File(destination.into(), e))?;
        for entry in fs::read_dir(source).map_err(|e| Error::File(source.into(), e))? {
            let entry = entry.map_err(|e| Error::File(source.into(), e))?;
            copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
        }
        Ok(())
    } else if metadata.is_file() {
        fs::copy(source, destination).map(|_| ())
    } else {
        Err(ErrorKind::InvalidInput.into())
    };
    copied.map_err(|e| Error::File(source.into(), e))
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::read_link(source).and_then(|target| std::os::unix::fs::symlink(target, destination))
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::{FileTypeExt, symlink_dir, symlink_file};

    let target = fs::read_link(source)?;
    if fs::symlink_metadata(source)?.file_type().is_symlink_dir() {
        symlink_dir(target, destination)
    } else {
        symlink_file(target, destination)
    }
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
