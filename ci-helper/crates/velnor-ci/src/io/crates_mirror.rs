// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::cli::CratesMirrorArgs;
use crate::error::CiError;

const CONFIG_ENV: &str = "VELNOR_CARGO_CONFIG";
const WRAPPER_ENV: &str = "VELNOR_CARGO_WRAPPER";
const REAL_ENV: &str = "VELNOR_CARGO_REAL";
const MARKER: &str = "# velnor-ci crates-mirror owns this file\n";

pub(super) fn setup(args: &CratesMirrorArgs) -> Result<String, CiError> {
    std::fs::File::open(&args.cainfo).map_err(|e| {
        CiError::new(format!(
            "cannot read crates mirror CA {}: {e}",
            args.cainfo.display()
        ))
    })?;
    let directory = PathBuf::from(
        std::env::var_os("RUNNER_TEMP").ok_or_else(|| CiError::new("set RUNNER_TEMP"))?,
    )
    .join("velnor-cargo");
    std::fs::create_dir_all(&directory).map_err(|e| failed(&e))?;
    let directory = std::fs::canonicalize(directory).map_err(|e| failed(&e))?;
    let wrapper = directory.join("cargo");
    let config = directory.join("config.toml");
    let quoted = |text: &str| serde_json::to_string(text).map_err(|e| CiError::new(e.to_string()));
    let text = format!(
        "{MARKER}[source.crates-io]\nreplace-with = \"velnor-crates-mirror\"\n[source.velnor-crates-mirror]\nregistry = {}\n[http]\ncainfo = {}\n",
        quoted(&args.registry)?,
        quoted(&args.cainfo.display().to_string())?
    );
    match std::fs::read_to_string(&config) {
        Ok(existing) if !existing.starts_with(MARKER) => {
            return Err(CiError::new(format!(
                "{} exists and is not owned by velnor-ci crates-mirror",
                config.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failed(&error)),
    }
    let staged = directory.join("config.toml.velnor-crates-mirror");
    std::fs::write(&staged, &text)
        .and_then(|()| std::fs::rename(staged, &config))
        .map_err(|e| failed(&e))?;
    let executable = std::env::current_exe().map_err(|e| failed(&e))?;
    std::fs::copy(executable, &wrapper).map_err(|e| failed(&e))?;
    super::append_to("GITHUB_PATH", &format!("{}\n", directory.display()))?;
    super::append_to(
        "GITHUB_ENV",
        &format!(
            "{CONFIG_ENV}={}\n{WRAPPER_ENV}={}\nCARGO={}\n",
            config.display(),
            wrapper.display(),
            wrapper.display()
        ),
    )?;
    Ok(format!(
        "Configured crates.io through {} in {}\n",
        args.registry,
        config.display()
    ))
}

pub(super) fn is_wrapper() -> bool {
    std::env::args_os().next().is_some_and(|arg| {
        Path::new(&arg)
            .file_name()
            .is_some_and(|name| name == "cargo")
    })
}

pub(super) fn run() -> ExitCode {
    match forward() {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn forward() -> Result<std::process::ExitStatus, CiError> {
    let wrapper = PathBuf::from(
        std::env::var_os(WRAPPER_ENV)
            .ok_or_else(|| CiError::new("missing VELNOR_CARGO_WRAPPER"))?,
    );
    let config =
        std::env::var_os(CONFIG_ENV).ok_or_else(|| CiError::new("missing VELNOR_CARGO_CONFIG"))?;
    let paths: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|path| Some(path.as_path()) != wrapper.parent())
        .collect();
    let mut args = std::env::args_os().skip(1).peekable();
    let selected = if args
        .peek()
        .is_some_and(|arg| arg.to_string_lossy().starts_with('+'))
    {
        args.next()
    } else {
        None
    };
    let cargo = if let (None, Some(executable)) = (selected.as_deref(), std::env::var_os(REAL_ENV))
    {
        ResolvedCargo {
            executable: PathBuf::from(executable),
            toolchain: None,
        }
    } else {
        let cargo = paths
            .iter()
            .map(|path| path.join("cargo"))
            .find(|path| {
                path.is_file()
                    && is_executable(path)
                    && std::fs::canonicalize(path).is_ok_and(|path| path != wrapper)
            })
            .ok_or_else(|| CiError::new("cannot find cargo outside the mirror wrapper"))?;
        actual_cargo(cargo, selected.as_deref())?
    };
    let cargo = ResolvedCargo {
        executable: std::fs::canonicalize(cargo.executable).map_err(|e| failed(&e))?,
        toolchain: cargo.toolchain,
    };
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
    command
        .arg("--config")
        .arg(config)
        .args(args)
        .env("PATH", path)
        .env(REAL_ENV, cargo.executable)
        .env("CARGO", wrapper)
        .status()
        .map_err(|e| failed(&e))
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
}

fn actual_cargo(
    cargo: PathBuf,
    selected: Option<&std::ffi::OsStr>,
) -> Result<ResolvedCargo, CiError> {
    let cargo = if selected.is_none()
        && let Some(mise) = proxy(&cargo, "mise")?
    {
        PathBuf::from(query(Command::new(mise).args(["which", "cargo"]))?)
    } else {
        cargo
    };
    let rustup = proxy(&cargo, "rustup")?;
    if selected.is_none() && rustup.is_none() {
        return Ok(ResolvedCargo {
            executable: cargo,
            toolchain: None,
        });
    }
    let rustup = rustup.unwrap_or_else(|| PathBuf::from("rustup"));
    let toolchain = if let Some(selected) = selected {
        selected
            .to_string_lossy()
            .trim_start_matches('+')
            .to_owned()
    } else {
        let active = query(Command::new(&rustup).args(["show", "active-toolchain"]))?;
        let toolchain = active
            .rsplit_once(" (")
            .map_or(active.as_str(), |(name, _)| name);
        if toolchain.is_empty() {
            return Err(CiError::new("Rustup returned no active toolchain"));
        }
        toolchain.to_owned()
    };
    let executable = PathBuf::from(query(
        Command::new(rustup)
            .env("RUSTUP_TOOLCHAIN", &toolchain)
            .args(["which", "cargo"]),
    )?);
    Ok(ResolvedCargo {
        executable,
        toolchain: Some(toolchain.into()),
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

fn failed(error: &std::io::Error) -> CiError {
    CiError::new(format!("cannot configure or run the Cargo mirror: {error}"))
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
