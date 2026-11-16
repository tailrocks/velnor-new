// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::error::CiError;

use super::crates_mirror::{CONFIG_ENV, REAL_ENV, WRAPPER_ENV, failed};
use super::crates_mirror_mise::{self as mise, LaunchPhase};

const RUSTUP_ENV: &str = "VELNOR_CARGO_RUSTUP";

pub(super) fn forward() -> Result<ExitStatus, CiError> {
    let wrapper = PathBuf::from(
        std::env::var_os(WRAPPER_ENV)
            .ok_or_else(|| CiError::new("missing VELNOR_CARGO_WRAPPER"))?,
    );
    let config =
        std::env::var_os(CONFIG_ENV).ok_or_else(|| CiError::new("missing VELNOR_CARGO_CONFIG"))?;
    let paths: Vec<_> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let phase = LaunchPhase::current()?;
    let initial = if matches!(phase, LaunchPhase::Outer) {
        let cargo = find_cargo(&paths, &wrapper, &phase)?;
        if let Some(executable) = proxy(&cargo, "mise")? {
            return mise::launch(&executable, &cargo, &wrapper, &args);
        }
        Some(cargo)
    } else {
        None
    };
    if matches!(phase, LaunchPhase::Activated)
        && let Some(status) = mise::after_activation(&wrapper, &args)?
    {
        return Ok(status);
    }
    let mut args = args.into_iter().peekable();
    let selected = if args
        .peek()
        .is_some_and(|arg| arg.to_string_lossy().starts_with('+'))
    {
        args.next()
    } else {
        None
    };
    let mut cargo = match phase {
        LaunchPhase::Pinned(executable) if selected.is_none() => ResolvedCargo {
            executable,
            toolchain: None,
            rustup: std::env::var_os(RUSTUP_ENV).map(PathBuf::from),
        },
        LaunchPhase::Pinned(executable) => actual_cargo(
            executable,
            selected.as_deref(),
            std::env::var_os(RUSTUP_ENV).map(PathBuf::from),
        )?,
        _ => actual_cargo(
            initial.map_or_else(|| find_cargo(&paths, &wrapper, &phase), Ok)?,
            selected.as_deref(),
            None,
        )?,
    };
    cargo.executable = std::fs::canonicalize(cargo.executable).map_err(|error| failed(&error))?;
    let executable = linked_cargo(&cargo.executable, &wrapper)?;
    let path = std::env::join_paths(
        wrapper
            .parent()
            .into_iter()
            .chain(paths.iter().map(PathBuf::as_path)),
    )
    .map_err(|error| CiError::new(format!("cannot configure Cargo PATH: {error}")))?;
    let mut command = Command::new(executable);
    if let Some(toolchain) = cargo.toolchain {
        command.env("RUSTUP_TOOLCHAIN", toolchain);
    }
    if let Some(rustup) = cargo.rustup {
        command.env(RUSTUP_ENV, rustup);
    } else {
        command.env_remove(RUSTUP_ENV);
    }
    mise::native_environment(&mut command);
    command
        .arg("--config")
        .arg(config)
        .args(args)
        .env("PATH", path)
        .env(REAL_ENV, cargo.executable)
        .env("CARGO", wrapper)
        .status()
        .map_err(|error| failed(&error))
}

fn find_cargo(paths: &[PathBuf], wrapper: &Path, phase: &LaunchPhase) -> Result<PathBuf, CiError> {
    let wrapper = std::fs::canonicalize(wrapper).map_err(|error| failed(&error))?;
    for path in paths {
        let cargo = path.join("cargo");
        if !cargo.is_file()
            || !is_executable(&cargo)
            || std::fs::canonicalize(&cargo).is_ok_and(|cargo| cargo == wrapper)
        {
            continue;
        }
        if !matches!(phase, LaunchPhase::Outer) && proxy(&cargo, "mise")?.is_some() {
            continue;
        }
        return Ok(cargo);
    }
    Err(CiError::new("cannot find cargo outside the mirror wrapper"))
}

fn linked_cargo(cargo: &Path, wrapper: &Path) -> Result<PathBuf, CiError> {
    let cargo = std::fs::canonicalize(cargo).map_err(|e| failed(&e))?;
    let directory = wrapper
        .parent()
        .ok_or_else(|| CiError::new("invalid Cargo wrapper path"))?
        .join(velnor_rust::sha256_hex(
            cargo.as_os_str().as_encoded_bytes(),
        ));
    std::fs::create_dir_all(&directory).map_err(|e| failed(&e))?;
    let executable = directory.join("velnor-cargo-real");
    if !executable.is_file() {
        match std::fs::hard_link(&cargo, &executable) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                let staged = directory.join(format!("velnor-cargo-real-{}", std::process::id()));
                std::fs::copy(&cargo, &staged)
                    .and_then(|_| std::fs::rename(staged, &executable))
                    .map_err(|e| failed(&e))?;
            }
        }
    }
    Ok(executable)
}

struct ResolvedCargo {
    executable: PathBuf,
    toolchain: Option<std::ffi::OsString>,
    rustup: Option<PathBuf>,
}

fn actual_cargo(
    cargo: PathBuf,
    selected: Option<&std::ffi::OsStr>,
    saved_rustup: Option<PathBuf>,
) -> Result<ResolvedCargo, CiError> {
    let rustup = proxy(&cargo, "rustup")?.or(saved_rustup);
    if selected.is_none() && rustup.is_none() {
        return Ok(ResolvedCargo {
            executable: cargo,
            toolchain: None,
            rustup: None,
        });
    }
    let rustup = rustup
        .or_else(|| {
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.join("rustup"))
                .find(|path| path.is_file() && is_executable(path))
        })
        .ok_or_else(|| CiError::new("cannot find Rustup to resolve Cargo toolchain"))?;
    let rustup = std::path::absolute(rustup).map_err(|error| failed(&error))?;
    let selected = selected.map(|selected| {
        selected
            .to_string_lossy()
            .trim_start_matches('+')
            .to_owned()
    });
    let mut command = Command::new(&rustup);
    if let Some(selected) = &selected {
        command.env("RUSTUP_TOOLCHAIN", selected);
    }
    let executable = PathBuf::from(query(command.args(["which", "cargo"]))?);
    let toolchain = selected
        .map(std::ffi::OsString::from)
        .or_else(|| {
            executable
                .parent()
                .and_then(Path::parent)
                .map(|path| path.as_os_str().to_owned())
        })
        .ok_or_else(|| CiError::new("Rustup returned an invalid Cargo toolchain path"))?;
    Ok(ResolvedCargo {
        executable,
        toolchain: Some(toolchain),
        rustup: Some(rustup),
    })
}

fn proxy(cargo: &Path, name: &str) -> Result<Option<PathBuf>, CiError> {
    let executable = std::fs::canonicalize(cargo).map_err(|e| failed(&e))?;
    if executable.file_stem().is_some_and(|stem| stem == name) {
        return Ok(Some(executable));
    }
    let paths = std::env::var_os("PATH").unwrap_or_default();
    let candidates = std::iter::once(cargo.with_file_name(name))
        .chain(std::env::split_paths(&paths).map(|path| path.join(name)));
    Ok(candidates.into_iter().find(|path| same_file(cargo, path)))
}

fn same_file(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (left.metadata(), right.metadata()) {
        (Ok(left), Ok(right)) => (left.dev(), left.ino()) == (right.dev(), right.ino()),
        _ => false,
    }
}

fn query(command: &mut Command) -> Result<String, CiError> {
    let output = command.output().map_err(|e| failed(&e))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "cannot resolve Cargo toolchain: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map(|path| path.trim().to_owned())
        .map_err(|e| CiError::new(e.to_string()))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}
