// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

pub use std::process::ExitCode as Exit;

use clap::Parser;
use velnor_model::{RepoPath, RepoView};
use velnor_render::OWNED_PATHS;
use velnor_rust::{DetectError, MetadataError, metadata_roots, reduce_metadata, wanted_files};

use crate::cli::{Cli, Command};
use crate::{Drift, drift, expected_files};

#[derive(Debug)]
enum Error {
    File(PathBuf, std::io::Error),
    Program(String, String),
    Repository(String),
    Metadata(MetadataError),
    Detect(DetectError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path, error) => write!(f, "{}: {error}", path.display()),
            Self::Program(program, stderr) => write!(f, "{program} failed: {}", stderr.trim()),
            Self::Repository(message) => f.write_str(message),
            Self::Metadata(error) => error.fmt(f),
            Self::Detect(error) => error.fmt(f),
        }
    }
}

#[must_use]
pub fn main() -> Exit {
    let cli = Cli::parse();
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
    let expected = expected_files(&read_view(&root)?).map_err(Error::Detect)?;
    let drifts = drift(&expected, &read_files(&root, &OWNED_PATHS)?);
    if cli.command == Command::Generate {
        for change in &drifts {
            let file = root.join(change.path().as_str());
            if let Some(content) = expected.get(change.path()) {
                if let Some(parent) = file.parent() {
                    fs::create_dir_all(parent).map_err(|e| Error::File(parent.into(), e))?;
                }
                fs::write(&file, content).map_err(|e| Error::File(file.clone(), e))?;
            } else {
                fs::remove_file(&file).map_err(|e| Error::File(file.clone(), e))?;
            }
        }
    }
    Ok(drifts)
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

fn read_view(root: &Path) -> Result<RepoView, Error> {
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
    for manifest in metadata_roots(&view).map_err(Error::Detect)? {
        let known = view.rust_metadata.iter().any(|(workspace, metadata)| {
            *workspace == manifest || metadata.members.iter().any(|m| m.manifest == manifest)
        });
        if known {
            continue;
        }
        let path = root.join(manifest.as_str());
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
        view.rust_metadata.insert(workspace, metadata);
    }
    Ok(view)
}

fn read_files(root: &Path, paths: &[RepoPath]) -> Result<BTreeMap<RepoPath, Vec<u8>>, Error> {
    let mut files = BTreeMap::new();
    for path in paths {
        let file = root.join(path.as_str());
        match fs::read(&file) {
            Ok(bytes) => {
                files.insert(path.clone(), bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::File(file, error)),
        }
    }
    Ok(files)
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
