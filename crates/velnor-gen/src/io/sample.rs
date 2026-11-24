// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use velnor_model::{RepoPath, RepoView, SAMPLES};
use velnor_rust::{cargo_files, is_plain_source, manifests, wanted_files};

use crate::expected_with_notes;

use super::{Error, read_view, regular_file, replace, toplevel};
use staged::StagedSample;

mod relocate;
mod staged;

pub use relocate::relocate_metadata;

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
    mut view: RepoView,
    mut metadata: BTreeMap<RepoPath, Vec<u8>>,
    out: &mut dyn Write,
) -> Result<Recording, Error> {
    let outside = |path: &RepoPath| !path.as_str().starts_with(&format!("{SAMPLES}/"));
    view.tracked_paths.retain(outside);
    view.files.retain(|path, _| outside(path));
    view.symlinks.retain(outside);
    metadata.retain(|workspace, _| outside(workspace));
    let (_, notes) = expected_with_notes(&view).map_err(Error::Generate)?;
    for note in notes {
        writeln!(out, "velnor-gen: {note}").ok();
    }
    let symlinks = view.symlinks.iter().cloned().collect();
    Ok(Recording {
        symlinks,
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
        .filter(|(path, bytes)| wanted.contains(path) && !is_plain_source(path, bytes))
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    pub view: BTreeMap<RepoPath, Vec<u8>>,
    pub manifests: Vec<RepoPath>,
    pub symlinks: Vec<RepoPath>,
    pub metadata: BTreeMap<RepoPath, Vec<u8>>,
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
    let mut links = String::new();
    for link in &recording.symlinks {
        links.push_str(link.as_str());
        links.push('\n');
    }
    files.push((
        sample_path(format!("{base}/symlinks.txt"))?,
        links.into_bytes(),
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
                Some("view" | "cargo-metadata" | "manifests.txt" | "symlinks.txt")
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
