// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const MIRROR_MARKER: &str = "# velnor-ci crates-mirror owns this file";

#[must_use]
pub fn is_mirror_wrapper(cargo: &Path) -> bool {
    let beside = cargo
        .parent()
        .into_iter()
        .flat_map(|dir| [Some(dir), dir.parent()]);
    beside.flatten().any(|dir| {
        std::fs::read_to_string(dir.join("config.toml"))
            .is_ok_and(|text| text.starts_with(MIRROR_MARKER))
    })
}

#[must_use]
pub fn is_mise_shim(cargo: &Path) -> bool {
    let Ok(executable) = std::fs::canonicalize(cargo) else {
        return false;
    };
    if executable.file_stem().is_some_and(|stem| stem == "mise") {
        return true;
    }
    let same = |other: &Path| match (cargo.metadata(), other.metadata()) {
        (Ok(left), Ok(right)) => (left.dev(), left.ino()) == (right.dev(), right.ino()),
        _ => false,
    };
    std::iter::once(cargo.with_file_name("mise"))
        .chain(search_path().map(|dir| dir.join("mise")))
        .any(|mise| same(&mise))
}

fn search_path() -> impl Iterator<Item = PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).collect::<Vec<_>>().into_iter()
}

fn candidates() -> impl Iterator<Item = PathBuf> {
    std::iter::once(PathBuf::from(env!("CARGO")))
        .chain(std::env::var_os("CARGO").map(PathBuf::from))
        .chain(search_path().map(|dir| dir.join("cargo")))
}

/// A cargo that is neither the crates-mirror wrapper nor a mise shim, however the
/// job's environment names it.
#[must_use]
pub fn real_cargo() -> PathBuf {
    candidates()
        .find(|cargo| cargo.is_file() && !is_mirror_wrapper(cargo) && !is_mise_shim(cargo))
        .unwrap_or_else(|| unreachable!("no cargo outside the crates mirror wrapper and mise"))
}

/// The crates-mirror cargo wrapper, for tests that intend to exercise it.
#[must_use]
pub fn mirror_cargo() -> PathBuf {
    candidates()
        .find(|cargo| cargo.is_file() && is_mirror_wrapper(cargo))
        .unwrap_or_else(|| unreachable!("no crates mirror cargo wrapper is configured"))
}

/// Drops the variables that make a child cargo resolve back to the mirror wrapper.
pub fn clean_cargo_env(command: &mut Command) -> &mut Command {
    command.env_remove("CARGO");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("VELNOR_CARGO_") {
            command.env_remove(name);
        }
    }
    command
}

/// `real_cargo()` as text, for argv and generated sources.
#[must_use]
pub fn real_cargo_str() -> String {
    real_cargo().to_string_lossy().into_owned()
}
