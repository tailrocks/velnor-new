// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_model::{RepoPath, RepoView};
use velnor_rust::{MetadataError, cargo_files, manifests, wanted_files};

use crate::expected_with_notes;

use super::{Error, read_view, regular_file, replace, toplevel};

pub const SAMPLES: &str = "generator/samples";
pub const SAMPLE_REPOSITORY: &str = "/repo";

pub fn record_sample(destination: &Path, checkout: &Path, name: &str) -> Result<(), Error> {
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
        .filter(|(path, _)| wanted.contains(path))
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    pub view: BTreeMap<RepoPath, Vec<u8>>,
    pub manifests: Vec<RepoPath>,
    pub symlinks: Vec<RepoPath>,
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
    relocate(&mut value, &[from, &encoded])?;
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
    let start = text.find("file://").map_or(0, |at| at + "file://".len());
    let path = &text[start..];
    for prefix in from {
        if let Some(tail) = path.strip_prefix(prefix)
            && (tail.is_empty()
                || tail.starts_with('/')
                || (start > 0 && tail.starts_with(['#', '?'])))
        {
            return format!("{}{SAMPLE_REPOSITORY}{tail}", &text[..start]);
        }
    }
    text.to_owned()
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

pub fn write_sample(root: &Path, name: &str, recording: &Recording) -> Result<(), Error> {
    let base = format!("{SAMPLES}/{}", sample_name(name)?);
    let mut files = Vec::new();
    for (path, bytes) in &recording.view {
        files.push((sample_path(format!("view/{path}"))?, bytes.clone()));
    }
    for (path, bytes) in &recording.metadata {
        files.push((
            sample_path(format!("cargo-metadata/{path}.json"))?,
            bytes.clone(),
        ));
    }
    let mut listing = String::new();
    for manifest in &recording.manifests {
        listing.push_str(manifest.as_str());
        listing.push('\n');
    }
    files.push((RepoPath::from_static("manifests.txt"), listing.into_bytes()));
    let mut links = String::new();
    for link in &recording.symlinks {
        links.push_str(link.as_str());
        links.push('\n');
    }
    files.push((RepoPath::from_static("symlinks.txt"), links.into_bytes()));
    let directories = [
        sample_path(format!("{base}/view"))?,
        sample_path(format!("{base}/cargo-metadata"))?,
    ];
    for (path, _) in &files {
        regular_file(root, &sample_path(format!("{base}/{path}"))?)?;
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
        replace(&prepared.join(path.as_str()), &bytes)?;
    }
    staged.publish(&root.join(base), existing.is_some())
}

struct StagedSample {
    path: PathBuf,
    cleanup: bool,
}

impl StagedSample {
    fn new(samples: &Path) -> Result<Self, Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let next = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = samples.join(format!(".velnor-gen-{}-{next}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        path,
                        cleanup: true,
                    });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(Error::File(path, error)),
            }
        }
    }

    fn publish(mut self, destination: &Path, exists: bool) -> Result<(), Error> {
        let previous = self.path.join("previous");
        if exists {
            fs::rename(destination, &previous).map_err(|e| Error::File(destination.into(), e))?;
        }
        if let Err(error) = fs::rename(self.path.join("new"), destination) {
            if exists && let Err(rollback) = fs::rename(&previous, destination) {
                self.cleanup = false;
                return Err(Error::Repository(format!(
                    "could not publish sample: {error}; could not restore it: {rollback}; original sample remains at {}",
                    previous.display()
                )));
            }
            return Err(Error::File(destination.into(), error));
        }
        Ok(())
    }
}

impl Drop for StagedSample {
    fn drop(&mut self) {
        if self.cleanup
            && let Err(error) = remove_staging(&self.path)
        {
            writeln!(
                std::io::stderr(),
                "velnor-gen: could not remove sample staging directory {}: {error}",
                self.path.display()
            )
            .ok();
        }
    }
}

fn remove_staging(path: &Path) -> std::io::Result<()> {
    writable_tree(path)?;
    fs::remove_dir_all(path)
}

fn writable_tree(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Ok(());
    }
    #[cfg(unix)]
    if metadata.is_dir() {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o700);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(windows)]
    if metadata.permissions().readonly() {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            writable_tree(&entry?.path())?;
        }
    }
    Ok(())
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
