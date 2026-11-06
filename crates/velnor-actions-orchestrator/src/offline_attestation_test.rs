// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    path::Path,
    process::{Command, Output},
};

pub(crate) fn run(
    test: &str,
    marker: &str,
    fixture: &Path,
    trusted_root: &Path,
) -> Result<Output, Box<dyn std::error::Error>> {
    run_inner(test, marker, fixture, trusted_root, false)
}

pub(crate) fn run_validated_worker(
    test: &str,
    marker: &str,
    fixture: &Path,
    trusted_root: &Path,
) -> Result<Output, Box<dyn std::error::Error>> {
    run_inner(test, marker, fixture, trusted_root, true)
}

fn run_inner(
    test: &str,
    marker: &str,
    fixture: &Path,
    trusted_root: &Path,
    worker: bool,
) -> Result<Output, Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let mise_data = std::env::var_os("MISE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .map(|value| std::path::PathBuf::from(value).join("mise"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|value| std::path::PathBuf::from(value).join(".local/share/mise"))
        })
        .ok_or("missing pinned tool data")?;
    let executable = std::env::current_exe()?;
    let mut command = offline_command(&executable)?;
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").ok_or("missing PATH")?)
        .env("HOME", home.path())
        .env("TMPDIR", home.path())
        .env("MISE_DATA_DIR", mise_data)
        .env(marker, "1")
        .env("VELNOR_ATTESTATION_TEST_DIR", fixture)
        .env("VELNOR_TEST_ATTESTATION_ROOT", trusted_root)
        .env("GH_TOKEN", "offline-fixture")
        .args(["--exact", test]);
    if worker {
        for name in [
            "VELNOR_GH_FIXTURE_WORKER",
            "VELNOR_GH_FIXTURE_TARGET",
            "VELNOR_GH_FIXTURE_SCENARIO",
            "VELNOR_GH_FIXTURE_NONCE",
            "VELNOR_GH_FIXTURE_CACHE_KEY",
            "VELNOR_GH_FIXTURE_ARCH",
            "VELNOR_GH_TEST_TLS_ROOT",
        ] {
            command.env(
                name,
                std::env::var_os(name).ok_or("missing validated worker context")?,
            );
        }
    }
    command.output().map_err(Into::into)
}

#[cfg(target_os = "macos")]
fn offline_command(executable: &Path) -> Result<Command, Box<dyn std::error::Error>> {
    if !Path::new("/usr/bin/sandbox-exec").is_file() {
        return Err("network sandbox unavailable".into());
    }
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command.args(["-p", "(version 1)(allow default)(deny network*)"]);
    command.arg(executable);
    Ok(command)
}

#[cfg(target_os = "linux")]
fn offline_command(executable: &Path) -> Result<Command, Box<dyn std::error::Error>> {
    if !Path::new("/usr/bin/python3").is_file() {
        return Err("network sandbox unavailable".into());
    }
    let mut command = Command::new("/usr/bin/python3");
    command.arg(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures/no_network.py"),
    );
    command.arg(executable);
    Ok(command)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn offline_command(_: &Path) -> Result<Command, Box<dyn std::error::Error>> {
    Err("network isolation unavailable on this platform".into())
}
