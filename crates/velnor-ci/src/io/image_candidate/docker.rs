// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::process::{Runner, clean_environment};
use super::{DOCKERFILE, fail, regular};
use crate::error::CiError;
use crate::image_candidate::build::buildx_arguments;
use crate::image_candidate::hashing::Hashing;
use crate::image_candidate::lock::{Lock, banner};

const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
const BUILD_TIMEOUT: Duration = Duration::from_mins(30);
pub(super) const BINARY_LIMIT: u64 = 1 << 30;
const PLUGIN: &str = "docker-buildx";
const SYSTEM_PLUGINS: &[&str] = &[
    "/usr/local/lib/docker/cli-plugins",
    "/usr/local/libexec/docker/cli-plugins",
    "/usr/lib/docker/cli-plugins",
    "/usr/libexec/docker/cli-plugins",
];

pub(super) struct Isolated {
    pub(super) config: PathBuf,
    pub(super) home: PathBuf,
}

pub(super) fn hash_file(path: &Path) -> Result<String, CiError> {
    let unreadable = |error: io::Error| CiError::new(format!("{}: {error}", path.display()));
    let mut hashing = Hashing::new(File::open(path).map_err(unreadable)?, BINARY_LIMIT);
    io::copy(&mut hashing, &mut io::sink()).map_err(unreadable)?;
    Ok(hashing.finish().0)
}

fn user_plugins() -> Option<PathBuf> {
    let config = std::env::var_os("DOCKER_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".docker"))
        })?;
    Some(config.join("cli-plugins"))
}

pub(super) fn plugin_source(explicit: Option<&Path>) -> Result<PathBuf, CiError> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    user_plugins()
        .into_iter()
        .chain(SYSTEM_PLUGINS.iter().map(PathBuf::from))
        .map(|directory| directory.join(PLUGIN))
        .find(|candidate| fs::symlink_metadata(candidate).is_ok())
        .ok_or_else(|| CiError::new("No docker-buildx plugin was found; pass --buildx."))
}

pub(super) fn isolate(source: &Path, lock: &Lock, context: &Path) -> Result<Isolated, CiError> {
    let Some(locked) = lock.buildx_sha256.as_deref() else {
        return fail("runner.lock: buildx_sha256 is missing");
    };
    if regular(source, "Docker Buildx plugin")? > BINARY_LIMIT {
        return fail("Docker Buildx plugin exceeds its size limit.");
    }
    let config = context.join("docker-config");
    let plugins = config.join("cli-plugins");
    let home = context.join("home");
    let copy = plugins.join(PLUGIN);
    let failed = |error: io::Error| CiError::new(format!("{}: {error}", context.display()));
    fs::create_dir(&config).map_err(failed)?;
    fs::create_dir(&plugins).map_err(failed)?;
    fs::copy(source, &copy).map_err(failed)?;
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o755)).map_err(failed)?;
    if hash_file(&copy)? != locked {
        return fail("Docker Buildx binary is not the same as the value in .velnor/runner.lock.");
    }
    Ok(Isolated { config, home })
}

fn docker_command(docker: &Path, isolated: &Isolated) -> Command {
    let mut command = Command::new(docker);
    clean_environment(&mut command)
        .env("DOCKER_CONFIG", &isolated.config)
        .env("HOME", &isolated.home);
    command
}

pub(super) fn version(
    runner: &Runner,
    docker: &Path,
    isolated: &Isolated,
    lock: &Lock,
) -> Result<String, CiError> {
    let mut command = docker_command(docker, isolated);
    command.args(["buildx", "version"]);
    banner(&runner.text(&mut command, QUERY_TIMEOUT)?, lock)
}

pub(super) fn build(
    runner: &Runner,
    docker: &Path,
    root: &Path,
    context: &Path,
    isolated: &Isolated,
    archive: &Path,
    commit: &str,
) -> Result<(), CiError> {
    let dockerfile = context.join("Dockerfile");
    fs::copy(root.join(DOCKERFILE), &dockerfile)
        .map_err(|error| CiError::new(format!("{}: {error}", context.display())))?;
    let mut command = docker_command(docker, isolated);
    command
        .args(buildx_arguments(&dockerfile, archive, context, commit))
        .current_dir(root);
    runner.run(&mut command, BUILD_TIMEOUT)
}
