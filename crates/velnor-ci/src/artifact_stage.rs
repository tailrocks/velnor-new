// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_model::{FILESYSTEM_CASE, path_matches, secret_path, segment_matches, selects};

use crate::cli::ArtifactsStageArgs;
use crate::error::CiError;
use crate::escape::escape;

const PREFIX: &str = "velnor-ci artifacts stage";
const WORKSPACE_VARIABLE: &str = "GITHUB_WORKSPACE";
const REFUSED_NAME_CHARACTERS: &[char] = &[':', '"', '<', '>', '|', '*', '?', '\r', '\n'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    HardLink,
    Link,
    Name,
    Secret,
    Special,
    Unreadable,
}

impl Skip {
    fn reason(self) -> &'static str {
        match self {
            Self::HardLink => {
                "a file with more than one hard link, which may be a second name for a file outside the workspace"
            }
            Self::Name => {
                "a name with a character that actions/upload-artifact rejects (one of : \" < > | * ? or a line break)"
            }
            Self::Link => {
                "a symbolic link, which the upload could follow out of the workspace; only regular files are staged"
            }
            Self::Secret => {
                "a path with a part that looks like a secret (a .env file, a key, a token, a password or credentials)"
            }
            Self::Special => "not a regular file or directory (a device, a socket or a pipe)",
            Self::Unreadable => "a file that could not be read",
        }
    }
}

#[derive(Debug, Default)]
pub struct Staged {
    pub copied: BTreeSet<PathBuf>,
    pub skipped: BTreeMap<PathBuf, Skip>,
}

#[must_use]
pub fn run(args: &ArtifactsStageArgs) -> ExitCode {
    let root = std::env::var_os(WORKSPACE_VARIABLE)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let Some(root) = root else {
        eprintln!("{PREFIX}: no workspace directory");
        return ExitCode::FAILURE;
    };
    match stage(&root, &args.paths, &args.into) {
        Ok(staged) => {
            for (path, skip) in &staged.skipped {
                println!(
                    "::warning::{PREFIX}: skipped {}: {}",
                    escape(&path.display().to_string(), false),
                    skip.reason()
                );
            }
            println!(
                "{PREFIX}: {} files staged, {} skipped, from {} paths",
                staged.copied.len(),
                staged.skipped.len(),
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

struct Walk<'a> {
    root: PathBuf,
    into: &'a Path,
    parts: Vec<&'a str>,
    staged: &'a mut Staged,
}

pub fn stage(root: &Path, patterns: &[String], into: &Path) -> Result<Staged, CiError> {
    fresh(into)?;
    let real = fs::canonicalize(root).map_err(|error| failure("read", root, root, &error))?;
    let mut staged = Staged::default();
    for pattern in patterns {
        let mut walk = Walk {
            root: real.clone(),
            into,
            parts: pattern.split('/').filter(|part| !part.is_empty()).collect(),
            staged: &mut staged,
        };
        walk.visit(&real, &[], false)?;
    }
    Ok(staged)
}

fn fresh(into: &Path) -> Result<(), CiError> {
    match fs::symlink_metadata(into) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(into),
        Ok(_) => fs::remove_file(into),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
    .and_then(|()| fs::create_dir_all(into))
    .map_err(|error| failure("create", into, into, &error))
}

fn refused_name(segments: &[&str]) -> bool {
    segments.iter().any(|segment| {
        segment
            .chars()
            .any(|c| REFUSED_NAME_CHARACTERS.contains(&c))
    })
}

fn hidden(segments: &[&str]) -> bool {
    segments.iter().any(|segment| segment.starts_with('.'))
}

fn leads(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern.split_first(), path.split_first()) {
        (_, None) | (Some((&"**", _)), _) => true,
        (None, Some(_)) => false,
        (Some((head, tail)), Some((first, rest))) => {
            segment_matches(head, first, FILESYSTEM_CASE) && leads(tail, rest)
        }
    }
}

impl Walk<'_> {
    fn visit(&mut self, directory: &Path, within: &[String], inside: bool) -> Result<(), CiError> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) if !within.is_empty() => {
                self.skip(within, Skip::Unreadable);
                return Ok(());
            }
            Err(error) => return Err(failure("read", &self.root, directory, &error)),
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let mut relative = within.to_vec();
            relative.push(name);
            let segments: Vec<&str> = relative.iter().map(String::as_str).collect();
            let hit = inside || path_matches(&self.parts, &segments, FILESYSTEM_CASE);
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                if hit || self.selected(&segments) {
                    self.skip(&relative, Skip::Link);
                }
            } else if kind.is_dir() {
                if (hit || leads(&self.parts, &segments)) && !hidden(&segments) {
                    if secret_path(&segments) {
                        if hit {
                            self.skip(&relative, Skip::Secret);
                        }
                    } else {
                        self.visit(&entry.path(), &relative, hit)?;
                    }
                }
            } else if hit || self.selected(&segments) {
                self.file(&entry.path(), &relative, kind.is_file())?;
            }
        }
        Ok(())
    }

    fn selected(&self, segments: &[&str]) -> bool {
        selects(&self.parts, segments, FILESYSTEM_CASE)
    }

    fn skip(&mut self, relative: &[String], skip: Skip) {
        self.staged.skipped.insert(relative.iter().collect(), skip);
    }

    fn file(&mut self, path: &Path, relative: &[String], regular: bool) -> Result<(), CiError> {
        let segments: Vec<&str> = relative.iter().map(String::as_str).collect();
        if hidden(&segments) {
            return Ok(());
        }
        if !regular {
            self.skip(relative, Skip::Special);
            return Ok(());
        }
        if secret_path(&segments) {
            self.skip(relative, Skip::Secret);
            return Ok(());
        }
        let expected: PathBuf = self.root.join(relative.iter().collect::<PathBuf>());
        if fs::canonicalize(path).ok().as_ref() != Some(&expected) {
            self.skip(relative, Skip::Link);
            return Ok(());
        }
        if refused_name(&segments) {
            self.skip(relative, Skip::Name);
            return Ok(());
        }
        let source = match open(path) {
            Ok(Some(source)) => source,
            Ok(None) => {
                self.skip(relative, Skip::Link);
                return Ok(());
            }
            Err(_) => {
                self.skip(relative, Skip::Unreadable);
                return Ok(());
            }
        };
        if !source.metadata().is_ok_and(|meta| meta.nlink() == 1) {
            self.skip(relative, Skip::HardLink);
            return Ok(());
        }
        self.copy(source, relative)
    }

    fn copy(&mut self, mut source: File, relative: &[String]) -> Result<(), CiError> {
        let target = self.into.join(relative.iter().collect::<PathBuf>());
        let parent = target.parent().unwrap_or(self.into);
        fs::create_dir_all(parent)
            .and_then(|()| File::create(&target))
            .and_then(|mut out| std::io::copy(&mut source, &mut out))
            .map_err(|error| failure("copy to", self.into, &target, &error))?;
        self.staged.copied.insert(relative.iter().collect());
        Ok(())
    }
}

fn open(path: &Path) -> std::io::Result<Option<File>> {
    let before = fs::symlink_metadata(path)?;
    if !before.is_file() {
        return Ok(None);
    }
    let source = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let after = source.metadata()?;
    let same = after.is_file() && after.dev() == before.dev() && after.ino() == before.ino();
    Ok(same.then_some(source))
}

fn failure(action: &str, root: &Path, path: &Path, error: &std::io::Error) -> CiError {
    let shown = path.strip_prefix(root).unwrap_or(path);
    CiError::new(format!(
        "{PREFIX}: cannot {action} {}: {error}",
        escape(&shown.display().to_string(), false)
    ))
}
