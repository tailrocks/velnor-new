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
            Self::Metadata(error) => error.fmt(f),
            Self::MetadataFile(error) => error.fmt(f),
            Self::Detect(error) => error.fmt(f),
            Self::Generate(error) => error.fmt(f),
            Self::Lock(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {}
