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

use clap::{CommandFactory, FromArgMatches};
use velnor_model::{RepoFacts, RepoPath, RepoView};
use velnor_render::OWNED_PATHS;
use velnor_rust::{
    DetectError, LOCK_PATH, LockError, MetadataError, detect, metadata_roots, pin_lock,
    reduce_metadata, release_version, sums_url, wanted_files,
};

mod sample;

use crate::cli::{Cli, Command};
use crate::{Drift, GenerateError, check_tag, drift, expected_files, require_compatible};
pub use sample::{
    Recording, SAMPLE_REPOSITORY, SAMPLES, record_sample, relocate_metadata, write_sample,
};

#[derive(Debug)]
pub enum Error {
    File(PathBuf, std::io::Error),
    NotRegular(RepoPath),
    Foreign(RepoPath),
    Program(String, String),
    Repository(String),
    Metadata(MetadataError),
    Detect(DetectError),
    Generate(GenerateError),
    Lock(LockError),
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
            Self::Generate(error) => error.fmt(f),
            Self::Lock(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

#[must_use]
pub fn version_text() -> String {
    format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("VELNOR_COMMIT").unwrap_or("unknown")
    )
}

pub fn parse_cli<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = Cli::command()
        .version(version_text())
        .try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

fn tag_verdict(tag: &str) -> Exit {
    match check_tag(env!("CARGO_PKG_VERSION"), tag) {
        Ok(message) => {
            println!("{message}");
            Exit::SUCCESS
        }
        Err(message) => {
            eprintln!("velnor-gen: {message}");
            Exit::from(1)
        }
    }
}

fn finish(result: Result<String, Error>) -> Exit {
    match result {
        Ok(message) => {
            println!("{message}");
            Exit::SUCCESS
        }
        Err(error) => {
            eprintln!("velnor-gen: {error}");
            Exit::from(2)
        }
    }
}

#[must_use]
pub fn main() -> Exit {
    let cli = match parse_cli(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    match &cli.command {
        Command::RecordSample { checkout, name } => {
            return finish(
                record_sample(&cli.root, checkout, name)
                    .map(|()| format!("recorded {SAMPLES}/{name}")),
            );
        }
        Command::PinHelper { version } => return finish(pin_helper(&cli.root, version)),
        Command::CheckTag { tag } => return tag_verdict(tag),
        Command::Generate | Command::Check => {}
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

fn pin_helper(root: &Path, version: &str) -> Result<String, Error> {
    let version = release_version(version).map_err(Error::Lock)?;
    require_compatible(&version).map_err(Error::Generate)?;
    let root = toplevel(root)?;
    let url = sums_url(&version);
    let sums = program(
        &root,
        "curl",
        &[
            OsStr::new("--fail"),
            OsStr::new("--silent"),
            OsStr::new("--show-error"),
            OsStr::new("--location"),
            OsStr::new(&url),
        ],
    )?;
    let lock = pin_lock(version.as_str(), &String::from_utf8_lossy(&sums)).map_err(Error::Lock)?;
    write_lock(&root, &lock)?;
    program(
        &root,
        "git",
        &[
            OsStr::new("add"),
            OsStr::new("--"),
            OsStr::new(LOCK_PATH.as_str()),
        ],
    )?;
    Ok(format!(
        "pinned velnor-ci and velnor-gen {version} in {LOCK_PATH} and staged it"
    ))
}

pub fn write_lock(root: &Path, lock: &str) -> Result<(), Error> {
    regular_file(root, &LOCK_PATH)?;
    replace(&root.join(LOCK_PATH.as_str()), lock.as_bytes())
}

fn execute(cli: &Cli) -> Result<Vec<Drift>, Error> {
    let root = toplevel(&cli.root)?;
    let expected = expected_files(&read_view(&root, false)?.0).map_err(Error::Generate)?;
    let drifts = drift(&expected, &read_files(&root, &OWNED_PATHS)?);
    if let Some(Drift::Foreign(path)) = drifts.iter().find(|d| matches!(d, Drift::Foreign(_))) {
        return Err(Error::Foreign(path.clone()));
    }
    if cli.command == Command::Generate {
        apply(&root, &expected, &drifts)?;
    }
    Ok(drifts)
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
    regular_file(
        root,
        &manifest.sibling(&RepoPath::from_static("Cargo.lock")),
    )?;
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

pub fn read_facts(path: &Path) -> Result<RepoFacts, Error> {
    detect(&read_view(&toplevel(path)?, false)?.0).map_err(Error::Detect)
}

type Recorded = (RepoView, BTreeMap<RepoPath, Vec<u8>>);

fn read_view(root: &Path, keep_metadata: bool) -> Result<Recorded, Error> {
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
        if keep_metadata {
            recorded.insert(workspace.clone(), output);
        }
        view.rust_metadata.insert(workspace, metadata);
    }
    let roots: Vec<RepoPath> = view.rust_metadata.keys().cloned().collect();
    view.files.extend(read_files(root, &roots)?);
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
