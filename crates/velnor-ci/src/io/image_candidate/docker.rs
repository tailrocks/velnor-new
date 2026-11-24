// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::process;
use super::{DOCKERFILE, fail};
use crate::error::CiError;
use crate::image_candidate::build::buildx_arguments;
use crate::image_candidate::hashing::Hashing;
use crate::image_candidate::lock::{self, Lock};

const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
const BUILD_TIMEOUT: Duration = Duration::from_mins(30);
const BINARY_LIMIT: u64 = 1 << 30;
const PLUGIN_FORMAT: &str = "{{json .ClientInfo.Plugins}}";

pub(super) struct Buildx {
    pub(super) banner: String,
    pub(super) binary: PathBuf,
}

fn hash_file(path: &Path) -> Result<String, CiError> {
    let unreadable = |error: io::Error| CiError::new(format!("{}: {error}", path.display()));
    let mut hashing = Hashing::new(File::open(path).map_err(unreadable)?, BINARY_LIMIT);
    io::copy(&mut hashing, &mut io::sink()).map_err(unreadable)?;
    Ok(hashing.finish().0)
}

fn query(docker: &str, arguments: &[&str]) -> Result<String, CiError> {
    let mut command = Command::new(docker);
    command.args(arguments);
    process::text(process::clean_environment(&mut command), QUERY_TIMEOUT)
}

pub(super) fn buildx(docker: &str, lock: &Lock) -> Result<Buildx, CiError> {
    let info = query(docker, &["info", "--format", PLUGIN_FORMAT])?;
    let plugin = lock::plugin(&info)?;
    let regular = fs::symlink_metadata(&plugin.path).is_ok_and(|meta| meta.is_file());
    if !regular {
        return fail("Docker Buildx plugin path is not a regular file.");
    }
    if hash_file(&plugin.path)? != lock.buildx_sha256 {
        return fail("Docker Buildx binary is not the same as the value in .velnor/runner.lock.");
    }
    if plugin.version.as_deref() != Some(format!("v{}", lock.buildx_version).as_str()) {
        return fail(
            "Docker Buildx plug-in metadata is not the same as the value in .velnor/runner.lock.",
        );
    }
    let banner = lock::banner(&query(docker, &["buildx", "version"])?, lock)?;
    Ok(Buildx {
        banner,
        binary: plugin.path,
    })
}

pub(super) fn build(
    docker: &str,
    root: &Path,
    context: &Path,
    archive: &Path,
    plugin: &Buildx,
    lock: &Lock,
) -> Result<(), CiError> {
    let context_error = |error: io::Error| CiError::new(format!("{}: {error}", context.display()));
    let dockerfile = context.join("Dockerfile");
    fs::copy(root.join(DOCKERFILE), &dockerfile).map_err(context_error)?;
    let plugins = context.join("docker-config/cli-plugins");
    fs::create_dir_all(&plugins).map_err(context_error)?;
    let isolated = plugins.join("docker-buildx");
    fs::copy(&plugin.binary, &isolated).map_err(context_error)?;
    fs::set_permissions(&isolated, fs::Permissions::from_mode(0o755)).map_err(context_error)?;
    if hash_file(&isolated)? != lock.buildx_sha256 {
        return fail("Isolated Buildx binary is not the same as the value in .velnor/runner.lock.");
    }
    let mut command = Command::new(docker);
    command
        .args(buildx_arguments(&dockerfile, archive, context))
        .current_dir(root);
    process::clean_environment(&mut command).env("DOCKER_CONFIG", context.join("docker-config"));
    process::run(&mut command, BUILD_TIMEOUT)
}
