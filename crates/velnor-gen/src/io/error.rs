// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::path::PathBuf;

use velnor_model::RepoPath;
use velnor_rust::{DetectError, LockError, MetadataError, MetadataFileError};

use crate::{Drift, GenerateError};

#[derive(Debug)]
pub enum Error {
    File(PathBuf, std::io::Error),
    NotRegular(RepoPath),
    Foreign(RepoPath),
    Program(String, String),
    Repository(String),
    UntrackedConfig(RepoPath),
    Metadata(MetadataError),
    MetadataFile(MetadataFileError),
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
            Self::UntrackedConfig(path) => write!(
                f,
                "{path} exists but git does not track it, so velnor-gen would ignore it and generate defaults; run `git add {path}` to track it (a git-ignored file needs `git add -f`), or remove the file"
            ),
            Self::Metadata(error) => error.fmt(f),
            Self::MetadataFile(error) => error.fmt(f),
            Self::Detect(error) => error.fmt(f),
            Self::Generate(error) => error.fmt(f),
            Self::Lock(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {}
