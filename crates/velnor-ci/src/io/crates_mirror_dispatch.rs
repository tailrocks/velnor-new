// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::error::CiError;

use super::crates_mirror::{CONFIG_ENV, REAL_ENV, WRAPPER_ENV, failed};
use super::crates_mirror_args::with_mirror_config;
use super::crates_mirror_depth::wrapper_ancestors;
use super::crates_mirror_mise::{self as mise, LaunchPhase};

const RUSTUP_ENV: &str = "VELNOR_CARGO_RUSTUP";
const DEPTH_ENV: &str = "VELNOR_CARGO_DEPTH";
/// Nested Cargo runs (build scripts, `cargo` shims) one forwarded run may stack up
/// before the wrapper decides a `cargo` on PATH is calling back into it.
const MAX_DEPTH: u32 = 64;
const SAVED: &str = "velnor-cargo-real";
const RESERVED: &[&str] = &["cargo", "config.toml", "config.toml.velnor-crates-mirror"];

pub(super) fn forward() -> Result<ExitStatus, CiError> {
    // The environment counter can be dropped by a shim, so the process tree is also counted.
    let depth = std::env::var(DEPTH_ENV)
        .ok()
        .and_then(|depth| depth.parse::<u32>().ok())
        .unwrap_or(0)
        .max(wrapper_ancestors(MAX_DEPTH));
    if depth >= MAX_DEPTH {
        return Err(CiError::new(format!(
            "the Cargo mirror wrapper was entered {depth} times in one chain: a `cargo` found on PATH forwards back into the wrapper ({}); put the real cargo ahead of it or remove the shim",
            std::env::var(WRAPPER_ENV).unwrap_or_default()
        )));
    }
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
    let directory = linked_toolchain(&cargo.executable, &wrapper)?;
    let path = std::env::join_paths(
        std::iter::once(directory.as_path()).chain(paths.iter().map(PathBuf::as_path)),
    )
    .map_err(|error| CiError::new(format!("cannot configure Cargo PATH: {error}")))?;
    let mut command = Command::new(directory.join(SAVED));
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
        .args(with_mirror_config(&config, args.collect()))
        .env("PATH", path)
        .env(REAL_ENV, cargo.executable)
        .env(DEPTH_ENV, (depth + 1).to_string())
        .env("CARGO", directory.join("cargo"))
        .status()
        .map_err(|error| failed(&error))
}

fn find_cargo(paths: &[PathBuf], wrapper: &Path, phase: &LaunchPhase) -> Result<PathBuf, CiError> {
    for path in paths {
        let cargo = path.join("cargo");
        if !cargo.is_file() || !is_executable(&cargo) || same_file(&cargo, wrapper) {
            continue;
        }
        if !matches!(phase, LaunchPhase::Outer) && proxy(&cargo, "mise")?.is_some() {
            continue;
        }
        return Ok(cargo);
    }
    saved_cargo(wrapper).ok_or_else(|| CiError::new("cannot find cargo outside the mirror wrapper"))
}

fn saved_cargo(wrapper: &Path) -> Option<PathBuf> {
    let beside_self = std::env::current_exe()
        .ok()
        .and_then(|executable| std::fs::canonicalize(executable).ok())
        .and_then(|executable| executable.parent().map(|directory| directory.join(SAVED)));
    let beside_wrapper = wrapper.parent().map(|directory| directory.join(SAVED));
    [beside_self, beside_wrapper]
        .into_iter()
        .flatten()
        .find(|saved| saved.is_file())
}

fn linked_toolchain(cargo: &Path, wrapper: &Path) -> Result<PathBuf, CiError> {
    let parent = wrapper
        .parent()
        .ok_or_else(|| CiError::new("invalid Cargo wrapper path"))?;
    let directory = parent.join(velnor_rust::sha256_hex(
        cargo.as_os_str().as_encoded_bytes(),
    ));
    std::fs::create_dir_all(&directory).map_err(|error| failed(&error))?;
    save_executable(cargo, &directory.join(SAVED))?;
    save_executable(wrapper, &directory.join("cargo"))?;
    let bin = cargo
        .parent()
        .ok_or_else(|| CiError::new("invalid Cargo toolchain path"))?;
    stage_view(bin, &directory)?;
    stage_view_entry(&directory.join(SAVED), &parent.join(SAVED))?;
    Ok(directory)
}

fn save_executable(source: &Path, target: &Path) -> Result<(), CiError> {
    if target.is_file() {
        return Ok(());
    }
    match std::fs::hard_link(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(_) => {
            let name = target
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let staged =
                target.with_file_name(format!("{name}.velnor-save-{}", std::process::id()));
            std::fs::copy(source, &staged)
                .and_then(|_| std::fs::rename(staged, target))
                .map_err(|error| failed(&error))
        }
    }
}

fn stage_view(bin: &Path, target: &Path) -> Result<(), CiError> {
    let entries = std::fs::read_dir(bin).map_err(|error| failed(&error))?;
    for entry in entries {
        let entry = entry.map_err(|error| failed(&error))?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if RESERVED.contains(&text.as_ref())
            || text.starts_with("velnor-")
            || text.starts_with("mise-")
        {
            continue;
        }
        if !entry.metadata().map_err(|error| failed(&error))?.is_file() {
            continue;
        }
        stage_view_entry(&entry.path(), &target.join(name))?;
    }
    Ok(())
}

fn stage_view_entry(source: &Path, target: &Path) -> Result<(), CiError> {
    if same_file(source, target) {
        return Ok(());
    }
    let name = target
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let staged = target.with_file_name(format!(".{name}.velnor-view-{}", std::process::id()));
    std::fs::remove_file(&staged).ok();
    std::os::unix::fs::symlink(source, &staged).map_err(|error| failed(&error))?;
    std::fs::rename(&staged, target).map_err(|error| failed(&error))
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
