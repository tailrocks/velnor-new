// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::is_mirror_wrapper;

/// Makes a nested `cargo` child independent of the job's crates-mirror, so a test that clears or
/// alters the child's environment behaves the same on CI as locally. Call it after the child's
/// other environment edits (`env_clear` and the loop that copies variables back), because it sets
/// `PATH` and removes variables.
///
/// On CI the mirror wrapper (a copy of `velnor-ci` named `cargo`) sits first on `PATH`, and the job
/// exports `VELNOR_CARGO_WRAPPER`, `VELNOR_CARGO_CONFIG`, `CARGO_REGISTRIES_*`, `CARGO_SOURCE_*` and
/// points `CARGO` at it. A child that loses those variables but keeps the `PATH` entry fails with
/// `missing VELNOR_CARGO_WRAPPER` the moment cargo (or mbx's `cargo metadata` probe) resolves
/// `cargo` by name, and a child that keeps them replaces crates-io with the mirror ahead of the
/// repo's own config.
///
/// This removes the mirror's variables and `CARGO` when it names the wrapper, and drops every
/// `PATH` folder whose `cargo` is the wrapper (found from `VELNOR_CARGO_WRAPPER` and by its
/// marker), so the child resolves the real (mise mbx) cargo.
pub fn ci_safe_cargo_env(command: &mut Command) -> &mut Command {
    let wrapper = std::env::var_os("VELNOR_CARGO_WRAPPER").map(PathBuf::from);
    for (key, value) in std::env::vars_os() {
        let name = key.to_string_lossy();
        let mirror = name.starts_with("VELNOR_CARGO_")
            || name.starts_with("CARGO_REGISTRIES_")
            || name.starts_with("CARGO_SOURCE_")
            || (name == "CARGO" && wrapper.as_deref().is_some_and(|w| Path::new(&value) == w));
        if mirror {
            command.env_remove(&key);
        }
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    command.env("PATH", without_mirror(&path, wrapper.as_deref()))
}

fn without_mirror(path: &OsStr, wrapper: Option<&Path>) -> OsString {
    let folder = wrapper.and_then(Path::parent);
    let kept = std::env::split_paths(path)
        .filter(|entry| Some(entry.as_path()) != folder && !is_mirror_wrapper(&entry.join("cargo")));
    std::env::join_paths(kept).unwrap_or_else(|_| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::without_mirror;
    use std::path::{Path, PathBuf};

    fn kept(path: &[&str], wrapper: Option<&str>) -> Vec<PathBuf> {
        let joined = std::env::join_paths(path).unwrap_or_else(|error| unreachable!("{error}"));
        let out = without_mirror(&joined, wrapper.map(Path::new));
        std::env::split_paths(&out).collect()
    }

    #[test]
    fn the_wrapper_folder_leaves_the_path_and_the_rest_keeps_its_order() {
        let path = ["/runner/velnor-cargo", "/usr/bin", "/opt/rust/bin"];
        assert_eq!(
            kept(&path, Some("/runner/velnor-cargo/cargo")),
            [Path::new("/usr/bin"), Path::new("/opt/rust/bin")]
        );
        assert_eq!(kept(&path, None).len(), 3);
    }

    #[test]
    fn a_folder_holding_the_mirror_marker_leaves_the_path_without_the_wrapper_variable() {
        let folder = std::env::temp_dir().join(format!("velnor-ci-env-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap_or_else(|error| unreachable!("{error}"));
        std::fs::write(
            folder.join("config.toml"),
            "# velnor-ci crates-mirror owns this file\n",
        )
        .unwrap_or_else(|error| unreachable!("{error}"));
        let marked = folder.to_string_lossy().into_owned();
        let out = kept(&[&marked, "/usr/bin"], None);
        std::fs::remove_dir_all(&folder).ok();
        assert_eq!(out, [Path::new("/usr/bin")]);
    }
}
