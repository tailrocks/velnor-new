// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::error::CiError;

use super::crates_mirror::failed;

const PHASE_ENV: &str = "VELNOR_CARGO_MISE_PHASE";
const DATA_ENV: &str = "VELNOR_CARGO_MISE_DATA";
const SHIM_ENV: &str = "VELNOR_CARGO_MISE_SHIM";

pub(super) enum LaunchPhase {
    Outer,
    Activated,
    Wrapped,
    ReadyNative,
    Pinned(PathBuf),
}

impl LaunchPhase {
    pub(super) fn current() -> Result<Self, CiError> {
        match std::env::var(PHASE_ENV).as_deref() {
            Ok("activated") => Ok(Self::Activated),
            Ok("wrapped") => Ok(Self::Wrapped),
            Ok(_) => Err(CiError::new("invalid Cargo Mise launch phase")),
            Err(std::env::VarError::NotPresent) => {
                if let Some(cargo) = std::env::var_os("VELNOR_CARGO_REAL") {
                    Ok(Self::Pinned(cargo.into()))
                } else if std::env::var("__MISE_ACTIVE_WRAPPERS")
                    .ok()
                    .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
                    .is_some_and(|wrappers| wrappers.iter().any(|name| name == "cargo"))
                {
                    Ok(Self::ReadyNative)
                } else {
                    Ok(Self::Outer)
                }
            }
            Err(error) => Err(CiError::new(error.to_string())),
        }
    }
}

pub(super) fn launch(
    mise: &Path,
    shim: &Path,
    wrapper: &Path,
    args: &[OsString],
) -> Result<ExitStatus, CiError> {
    let original = data_directory(mise)?;
    let directory = wrapper
        .parent()
        .ok_or_else(|| CiError::new("invalid Cargo wrapper path"))?;
    let proxy = fresh_directory(directory)?;
    delegate_entries(&original, &proxy, "command-wrappers")?;
    let wrappers = proxy.join("command-wrappers");
    std::fs::create_dir_all(wrappers.join("bin")).map_err(|error| failed(&error))?;
    delegate_entries(&original.join("command-wrappers"), &wrappers, "bin")?;
    delegate_entries(
        &original.join("command-wrappers/bin"),
        &wrappers.join("bin"),
        "cargo",
    )?;
    let shim = if shim.is_absolute() {
        shim.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| failed(&error))?
            .join(shim)
    };
    Command::new(mise)
        .args(["exec", "--"])
        .arg(wrapper)
        .args(args)
        .env("MISE_DATA_DIR", &proxy)
        .env(DATA_ENV, &proxy)
        .env(PHASE_ENV, "activated")
        .env(SHIM_ENV, shim)
        .env("CARGO", wrapper)
        .status()
        .map_err(|error| failed(&error))
}

pub(super) fn after_activation(
    wrapper: &Path,
    args: &[OsString],
) -> Result<Option<ExitStatus>, CiError> {
    let data = PathBuf::from(
        std::env::var_os(DATA_ENV)
            .ok_or_else(|| CiError::new("missing activated Mise data directory"))?,
    );
    let cargo = data.join("command-wrappers/bin/cargo");
    if !cargo.is_file() {
        return Ok(None);
    }
    let staged = cargo.with_file_name(format!("cargo.velnor-{}", std::process::id()));
    std::os::unix::fs::symlink(wrapper, &staged)
        .and_then(|()| std::fs::rename(staged, cargo))
        .map_err(|error| failed(&error))?;
    let shim = original_shim().ok_or_else(|| CiError::new("missing original Mise Cargo shim"))?;
    Command::new(shim)
        .args(args)
        .env(PHASE_ENV, "wrapped")
        .env("MISE_DATA_DIR", data)
        .env("CARGO", wrapper)
        .status()
        .map(Some)
        .map_err(|error| failed(&error))
}

pub(super) fn original_shim() -> Option<PathBuf> {
    std::env::var_os(SHIM_ENV).map(PathBuf::from)
}

pub(super) fn native_environment(command: &mut Command) {
    command
        .env_remove(PHASE_ENV)
        .env_remove(SHIM_ENV)
        .env_remove(DATA_ENV);
}

fn fresh_directory(parent: &Path) -> Result<PathBuf, CiError> {
    for sequence in 0u64.. {
        let directory = parent.join(format!("mise-{}-{sequence}", std::process::id()));
        match std::fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(failed(&error)),
        }
    }
    Err(CiError::new("cannot allocate Mise Cargo directory"))
}

fn delegate_entries(source: &Path, destination: &Path, excluded: &str) -> Result<(), CiError> {
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(failed(&error)),
    };
    for entry in entries {
        let entry = entry.map_err(|error| failed(&error))?;
        if entry.file_name() != excluded {
            std::os::unix::fs::symlink(entry.path(), destination.join(entry.file_name()))
                .map_err(|error| failed(&error))?;
        }
    }
    Ok(())
}

fn data_directory(mise: &Path) -> Result<PathBuf, CiError> {
    #[derive(serde::Deserialize)]
    struct Doctor {
        dirs: Directories,
    }
    #[derive(serde::Deserialize)]
    struct Directories {
        data: PathBuf,
    }
    if let Some(path) = std::env::var_os("MISE_DATA_DIR").map(PathBuf::from)
        && path.is_absolute()
    {
        return Ok(path);
    }
    let output = Command::new(mise)
        .args(["doctor", "--json"])
        .output()
        .map_err(|error| failed(&error))?;
    let doctor: Doctor = serde_json::from_slice(&output.stdout)
        .map_err(|_| CiError::new("Mise doctor did not report a valid data directory"))?;
    if !doctor.dirs.data.is_absolute() {
        return Err(CiError::new(
            "Mise doctor reported a nonabsolute data directory",
        ));
    }
    Ok(doctor.dirs.data)
}
