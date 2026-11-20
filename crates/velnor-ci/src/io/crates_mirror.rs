// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli::CratesMirrorArgs;
use crate::error::CiError;

pub(super) const CONFIG_ENV: &str = "VELNOR_CARGO_CONFIG";
pub(super) const WRAPPER_ENV: &str = "VELNOR_CARGO_WRAPPER";
pub(super) const REAL_ENV: &str = "VELNOR_CARGO_REAL";
pub(super) const DISABLE_ENV: &str = "VELNOR_CRATES_MIRROR";
const MARKER: &str = "# velnor-ci crates-mirror owns this file\n";

pub(super) fn setup(args: &CratesMirrorArgs) -> Result<String, CiError> {
    let disabled = match std::env::var(DISABLE_ENV) {
        Ok(value) if value == "off" => true,
        Ok(value) if !value.is_empty() => {
            return Err(CiError::new(format!(
                "{DISABLE_ENV} must be unset or exactly off, not {value:?}"
            )));
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(CiError::new(format!(
                "{DISABLE_ENV} must be unset or exactly off"
            )));
        }
        Ok(_) | Err(std::env::VarError::NotPresent) => false,
    };
    // MBX is never skipped: a declaring repository routes every cargo through the forwarding
    // wrapper even when the mirror is off, and the wrapper refuses any other cargo at use time.
    // Setup runs before the tools install, so mise's wrapper may not exist yet here.
    let mbx = super::crates_mirror_mbx::declared_here()?;
    if disabled {
        let mut text = format!("crates mirror disabled by {DISABLE_ENV}=off\n");
        if mbx {
            let directory = runner_directory()?;
            let wrapper = directory.join("cargo");
            install_wrapper(&directory, &wrapper)?;
            super::append_to("GITHUB_PATH", &format!("{}\n", directory.display()))?;
            super::append_to(
                "GITHUB_ENV",
                &format!(
                    "{}={}\n{DISABLE_ENV}=off\n{WRAPPER_ENV}={}\nCARGO={}\n",
                    super::crates_mirror_mbx::ENV,
                    super::crates_mirror_mbx::VALUE,
                    wrapper.display(),
                    wrapper.display()
                ),
            )?;
            text.push_str(
                "MBX is declared: cargo is checked at use time, no mirror config written\n",
            );
        }
        return Ok(text);
    }
    std::fs::File::open(&args.cainfo).map_err(|e| {
        CiError::new(format!(
            "cannot read crates mirror CA {}: {e}",
            args.cainfo.display()
        ))
    })?;
    let directory = runner_directory()?;
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
    install_wrapper(&directory, &wrapper)?;
    super::append_to("GITHUB_PATH", &format!("{}\n", directory.display()))?;
    let mbx_line = if mbx {
        format!(
            "{}={}\n",
            super::crates_mirror_mbx::ENV,
            super::crates_mirror_mbx::VALUE
        )
    } else {
        String::new()
    };
    super::append_to(
        "GITHUB_ENV",
        &format!(
            "{CONFIG_ENV}={}\n{WRAPPER_ENV}={}\nCARGO={}\n{mbx_line}",
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

fn runner_directory() -> Result<PathBuf, CiError> {
    let directory = PathBuf::from(
        std::env::var_os("RUNNER_TEMP").ok_or_else(|| CiError::new("set RUNNER_TEMP"))?,
    )
    .join("velnor-cargo");
    std::fs::create_dir_all(&directory).map_err(|e| failed(&e))?;
    std::fs::canonicalize(directory).map_err(|e| failed(&e))
}

/// Copies velnor-ci in as the `cargo` the job reaches first; it forwards to the real cargo.
fn install_wrapper(directory: &Path, wrapper: &Path) -> Result<(), CiError> {
    let executable = std::env::current_exe().map_err(|e| failed(&e))?;
    let staged_wrapper = directory.join("cargo.velnor-crates-mirror");
    std::fs::copy(executable, &staged_wrapper)
        .and_then(|_| std::fs::rename(&staged_wrapper, wrapper))
        .map_err(|e| failed(&e))
}

pub(super) fn runner_config() -> Result<Option<PathBuf>, CiError> {
    let Some(path) = std::env::var_os(CONFIG_ENV).filter(|path| !path.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    let written = path.is_absolute()
        && std::fs::read_to_string(&path).is_ok_and(|text| is_written_by_setup(&text));
    if written {
        Ok(Some(path))
    } else {
        Err(CiError::new(format!(
            "{CONFIG_ENV} is {}, which is not a file crates-mirror wrote",
            path.display()
        )))
    }
}

fn is_written_by_setup(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let quoted = |line: &str, key: &str| {
        line.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(" = "))
            .is_some_and(|value| serde_json::from_str::<String>(value).is_ok())
    };
    matches!(
        lines.as_slice(),
        [marker, "[source.crates-io]", "replace-with = \"velnor-crates-mirror\"", "[source.velnor-crates-mirror]", registry, "[http]", cainfo]
            if format!("{marker}\n") == MARKER && quoted(registry, "registry") && quoted(cainfo, "cainfo")
    )
}

pub(super) fn is_wrapper() -> bool {
    std::env::args_os().next().is_some_and(|arg| {
        Path::new(&arg)
            .file_name()
            .is_some_and(|name| name == "cargo")
    })
}

pub(super) fn run() -> ExitCode {
    match super::crates_mirror_dispatch::forward() {
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

pub(super) fn failed(error: &std::io::Error) -> CiError {
    CiError::new(format!("cannot configure or run the Cargo mirror: {error}"))
}
