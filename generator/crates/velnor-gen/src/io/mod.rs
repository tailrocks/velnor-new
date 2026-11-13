// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Command as Process;

pub use std::process::ExitCode as Exit;

use clap::Parser;
use velnor_model::{RepoPath, RepoView};
use velnor_render::OWNED_PATHS;
use velnor_rust::{
    DetectError, MetadataError, manifests, metadata_roots, reduce_metadata, wanted_files,
};

use crate::cli::{Cli, Command};
use crate::{Drift, drift, expected_files};

#[derive(Debug)]
pub enum Error {
    File(PathBuf, std::io::Error),
    NotRegular(RepoPath),
    Foreign(RepoPath),
    Program(String, String),
    Repository(String),
    Metadata(MetadataError),
    Detect(DetectError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path, error) => write!(f, "{}: {error}", path.display()),
            Self::NotRegular(path) => write!(
                f,
                "{path} goes through a symbolic link or is not a regular file"
            ),
            Self::Foreign(path) => Drift::Foreign(path.clone()).fmt(f),
            Self::Program(program, stderr) => write!(f, "{program} failed: {}", stderr.trim()),
            Self::Repository(message) => f.write_str(message),
            Self::Metadata(error) => error.fmt(f),
            Self::Detect(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

#[must_use]
pub fn main() -> Exit {
    let cli = Cli::parse();
    if let Command::RecordSample { checkout, name } = &cli.command {
        return match record_sample(&cli.root, checkout, name) {
            Ok(()) => {
                println!("recorded {SAMPLES}/{name}");
                Exit::SUCCESS
            }
            Err(error) => {
                eprintln!("velnor-gen: {error}");
                Exit::from(2)
            }
        };
    }
    match execute(&cli) {
        Ok(drifts) if cli.command == Command::Check && !drifts.is_empty() => {
            for drift in &drifts {
                eprintln!("{drift}");
            }
            eprintln!("run `velnor-gen generate` to update the generated files");
            Exit::from(1)
        }
        Ok(drifts) => {
            if drifts.is_empty() {
                println!("generated files are up to date");
            }
            for drift in &drifts {
                match drift {
                    Drift::Changed(path) | Drift::Missing(path) => println!("wrote {path}"),
                    Drift::Stale(path) => println!("removed {path}"),
                    Drift::Foreign(path) => println!("kept {path}"),
                }
            }
            Exit::SUCCESS
        }
        Err(error) => {
            eprintln!("velnor-gen: {error}");
            Exit::from(2)
        }
    }
}

fn execute(cli: &Cli) -> Result<Vec<Drift>, Error> {
    let root = toplevel(&cli.root)?;
    let expected = expected_files(&read_view(&root)?.0).map_err(Error::Detect)?;
    let drifts = drift(&expected, &read_files(&root, &OWNED_PATHS)?);
    if let Some(Drift::Foreign(path)) = drifts.iter().find(|d| matches!(d, Drift::Foreign(_))) {
        return Err(Error::Foreign(path.clone()));
    }
    if cli.command == Command::Generate {
        apply(&root, &expected, &drifts)?;
    }
    Ok(drifts)
}

pub const SAMPLES: &str = "generator/samples";
pub const SAMPLE_REPOSITORY: &str = "/repo";

pub fn record_sample(destination: &Path, checkout: &Path, name: &str) -> Result<(), Error> {
    let name = sample_name(name)?;
    let destination = toplevel(destination)?;
    let checkout = toplevel(checkout)?;
    let (view, metadata) = read_view(&checkout)?;
    let recording = Recording {
        view: view.files,
        manifests: manifests(&view.tracked_paths),
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
        .ok_or_else(|| Error::Repository("the checkout path is not UTF-8".to_owned()))?;
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

fn relocate(value: &mut serde_json::Value, from: &[&str]) {
    match value {
        serde_json::Value::String(text) => {
            for prefix in from {
                *text = text.replace(prefix, SAMPLE_REPOSITORY);
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| relocate(v, from)),
        serde_json::Value::Object(map) => map.values_mut().for_each(|v| relocate(v, from)),
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
    for directory in &directories {
        if let Some(existing) = real_directory(root, directory)? {
            fs::remove_dir_all(&existing).map_err(|e| Error::File(existing, e))?;
        }
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

pub fn apply(
    root: &Path,
    expected: &BTreeMap<RepoPath, String>,
    drifts: &[Drift],
) -> Result<(), Error> {
    for change in drifts {
        let path = change.path();
        let existing = regular_file(root, path)?;
        match (change, expected.get(path), existing) {
            (Drift::Foreign(_), _, _) => return Err(Error::Foreign(path.clone())),
            (_, Some(content), _) => replace(&root.join(path.as_str()), content.as_bytes())?,
            (_, None, Some(file)) => {
                fs::remove_file(&file).map_err(|e| Error::File(file.clone(), e))?;
            }
            (_, None, None) => {}
        }
    }
    Ok(())
}

fn replace(file: &Path, content: &[u8]) -> Result<(), Error> {
    let parent = file
        .parent()
        .ok_or_else(|| Error::File(file.into(), ErrorKind::InvalidInput.into()))?;
    fs::create_dir_all(parent).map_err(|e| Error::File(parent.into(), e))?;
    let mut name = OsString::from(".");
    name.push(file.file_name().unwrap_or_default());
    name.push(".velnor-gen");
    let temporary = parent.join(name);
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != ErrorKind::NotFound => {
            return Err(Error::File(temporary, error));
        }
        _ => {}
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut handle| handle.write_all(content))
        .and_then(|()| fs::rename(&temporary, file))
        .map_err(|e| Error::File(file.into(), e))
}

pub fn manifest_file(root: &Path, manifest: &RepoPath) -> Result<PathBuf, Error> {
    regular_file(root, manifest)?.ok_or_else(|| Error::NotRegular(manifest.clone()))
}

fn toplevel(path: &Path) -> Result<PathBuf, Error> {
    let output = program(
        path,
        "git",
        &[OsStr::new("rev-parse"), OsStr::new("--show-toplevel")],
    )?;
    let text = String::from_utf8(output)
        .map_err(|_| Error::Repository("the repository path is not UTF-8".to_owned()))?;
    let root = PathBuf::from(text.trim_end_matches('\n'));
    fs::canonicalize(&root).map_err(|e| Error::File(root, e))
}

type Recorded = (RepoView, BTreeMap<RepoPath, Vec<u8>>);

fn read_view(root: &Path) -> Result<Recorded, Error> {
    let tracked_paths = program(root, "git", &[OsStr::new("ls-files"), OsStr::new("-z")])?
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let text = std::str::from_utf8(entry).map_err(|_| {
                Error::Repository(format!(
                    "tracked path {:?} is not UTF-8",
                    String::from_utf8_lossy(entry)
                ))
            })?;
            RepoPath::new(text).map_err(|e| Error::Repository(e.to_string()))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut view = RepoView {
        files: read_files(root, &wanted_files(&tracked_paths))?,
        tracked_paths,
        rust_metadata: BTreeMap::new(),
    };
    let mut recorded = BTreeMap::new();
    for manifest in metadata_roots(&view).map_err(Error::Detect)? {
        let known = view.rust_metadata.iter().any(|(workspace, metadata)| {
            *workspace == manifest || metadata.members.iter().any(|m| m.manifest == manifest)
        });
        if known {
            continue;
        }
        let path = manifest_file(root, &manifest)?;
        let directory = path.parent().unwrap_or(root);
        let output = program(
            directory,
            "cargo",
            &[
                OsStr::new("metadata"),
                OsStr::new("--format-version"),
                OsStr::new("1"),
                OsStr::new("--no-deps"),
                OsStr::new("--offline"),
                OsStr::new("--manifest-path"),
                path.as_os_str(),
            ],
        )?;
        let (workspace, metadata) = reduce_metadata(root, &output).map_err(Error::Metadata)?;
        recorded.insert(workspace.clone(), output);
        view.rust_metadata.insert(workspace, metadata);
    }
    Ok((view, recorded))
}

pub fn read_files(root: &Path, paths: &[RepoPath]) -> Result<BTreeMap<RepoPath, Vec<u8>>, Error> {
    let mut files = BTreeMap::new();
    for path in paths {
        if let Some(file) = regular_file(root, path)? {
            let bytes = fs::read(&file).map_err(|e| Error::File(file, e))?;
            files.insert(path.clone(), bytes);
        }
    }
    Ok(files)
}

fn regular_file(root: &Path, path: &RepoPath) -> Result<Option<PathBuf>, Error> {
    let parts: Vec<&str> = path.as_str().split('/').collect();
    let mut current = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(Error::File(current, error)),
        };
        let last = index + 1 == parts.len();
        if (last && !metadata.is_file()) || (!last && !metadata.is_dir()) {
            return Err(Error::NotRegular(path.clone()));
        }
    }
    Ok(Some(current))
}

fn program(directory: &Path, name: &str, args: &[&OsStr]) -> Result<Vec<u8>, Error> {
    let label = format!("{name} {}", args[0].display());
    let output = Process::new(name)
        .args(args)
        .current_dir(directory)
        .output()
        .map_err(|error| Error::Program(label.clone(), error.to_string()))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(Error::Program(
            label,
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}
