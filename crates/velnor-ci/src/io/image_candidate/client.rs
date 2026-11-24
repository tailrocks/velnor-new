// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::docker::{BINARY_LIMIT, hash_file};
use super::{fail, regular};
use crate::error::CiError;
use crate::image_candidate::lock::Lock;

const PROGRAM: &str = "docker";

pub(super) struct Client {
    copy: PathBuf,
    sha256: String,
}

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn resolve(requested: Option<&Path>) -> Result<PathBuf, CiError> {
    let found = match requested {
        Some(path) => path.to_path_buf(),
        None => std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths)
            .map(|directory| directory.join(PROGRAM))
            .find(|candidate| executable(candidate))
            .ok_or_else(|| CiError::new("No docker client was found on PATH; pass --docker."))?,
    };
    fs::canonicalize(&found).map_err(|error| CiError::new(format!("{}: {error}", found.display())))
}

impl Client {
    pub(super) fn pin(
        requested: Option<&Path>,
        root: &Path,
        lock: &Lock,
        staging: &Path,
    ) -> Result<Self, CiError> {
        let sha256 = lock
            .docker_client_sha256(std::env::consts::ARCH)?
            .to_owned();
        let source = resolve(requested)?;
        if source.starts_with(root) {
            return fail("Docker client must be outside the source tree.");
        }
        if regular(&source, "Docker client")? > BINARY_LIMIT {
            return fail("Docker client exceeds its size limit.");
        }
        let copy = staging.join(PROGRAM);
        let failed = |error: io::Error| CiError::new(format!("{}: {error}", staging.display()));
        fs::copy(&source, &copy).map_err(failed)?;
        fs::set_permissions(&copy, fs::Permissions::from_mode(0o755)).map_err(failed)?;
        let client = Self { copy, sha256 };
        client.checked()?;
        Ok(client)
    }

    pub(super) fn checked(&self) -> Result<&Path, CiError> {
        if hash_file(&self.copy)? != self.sha256 {
            return fail(
                "Docker client binary is not the same as the value in .velnor/runner.lock.",
            );
        }
        Ok(&self.copy)
    }
}
