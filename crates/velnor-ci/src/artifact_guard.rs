// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli::ArtifactsCheckArgs;
use crate::error::CiError;
use crate::escape::escape;

const PREFIX: &str = "velnor-ci artifacts check";
const WORKSPACE_VARIABLE: &str = "GITHUB_WORKSPACE";

#[must_use]
pub fn run(args: &ArtifactsCheckArgs) -> ExitCode {
    let root = std::env::var_os(WORKSPACE_VARIABLE)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let Some(root) = root else {
        eprintln!("{PREFIX}: no workspace directory");
        return ExitCode::FAILURE;
    };
    match sweep(&root, &args.paths) {
        Ok(removed) => {
            for path in &removed {
                println!(
                    "::warning::{PREFIX}: removed the symbolic link {} so that the upload cannot follow it out of the workspace",
                    escape(&path.display().to_string(), false)
                );
            }
            println!(
                "{PREFIX}: {} symbolic links removed from {} paths",
                removed.len(),
                args.paths.len()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

pub fn sweep(root: &Path, patterns: &[String]) -> Result<BTreeSet<PathBuf>, CiError> {
    let mut removed = BTreeSet::new();
    for pattern in patterns {
        let literal = pattern
            .split('/')
            .filter(|segment| !segment.is_empty())
            .take_while(|segment| !segment.contains('*'));
        let mut current = root.to_path_buf();
        let mut reached = true;
        for segment in literal {
            current.push(segment);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    unlink(root, &current, &mut removed)?;
                    reached = false;
                    break;
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    reached = false;
                    break;
                }
                Err(error) => return Err(failure("read", root, &current, &error)),
            }
        }
        if reached {
            walk(root, &current, &mut removed)?;
        }
    }
    Ok(removed)
}

fn walk(root: &Path, start: &Path, removed: &mut BTreeSet<PathBuf>) -> Result<(), CiError> {
    let mut pending = vec![start.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.raw_os_error() == Some(20) => continue,
            Err(error) => return Err(failure("read", root, &directory, &error)),
        };
        for entry in entries {
            let entry = entry.map_err(|error| failure("read", root, &directory, &error))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| failure("read", root, &path, &error))?;
            if kind.is_symlink() {
                unlink(root, &path, removed)?;
            } else if kind.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(())
}

fn unlink(root: &Path, path: &Path, removed: &mut BTreeSet<PathBuf>) -> Result<(), CiError> {
    fs::remove_file(path).map_err(|error| failure("remove", root, path, &error))?;
    removed.insert(relative(root, path));
    Ok(())
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn failure(action: &str, root: &Path, path: &Path, error: &std::io::Error) -> CiError {
    CiError::new(format!(
        "{PREFIX}: cannot {action} {}: {error}",
        escape(&relative(root, path).display().to_string(), false)
    ))
}
