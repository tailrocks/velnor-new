// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

fn search_path() -> impl Iterator<Item = PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).collect::<Vec<_>>().into_iter()
}

fn candidates() -> impl Iterator<Item = PathBuf> {
    std::iter::once(PathBuf::from(env!("CARGO")))
        .chain(std::env::var_os("CARGO").map(PathBuf::from))
        .chain(search_path().map(|dir| dir.join("cargo")))
}

/// The cargo a test runs: mise's command-wrapper cargo when the job has one (it runs cargo
/// through mbx, which every build must use), otherwise the first cargo on the PATH. Never the
/// crates-mirror wrapper, which the tests compare against.
#[must_use]
pub fn real_cargo() -> PathBuf {
    pick_real_cargo(candidates()).unwrap_or_else(|| {
        panic!("no cargo on the PATH other than the crates-mirror wrapper; run the test under mise")
    })
}

fn pick_real_cargo(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let usable: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|cargo| cargo.is_file() && !is_mirror_wrapper(cargo))
        .collect();
    usable
        .iter()
        .find(|cargo| is_mise_command_wrapper(cargo))
        .or_else(|| usable.first())
        .cloned()
}

/// mise's `command-wrappers/bin/cargo`, the one that runs cargo through mbx.
fn is_mise_command_wrapper(cargo: &Path) -> bool {
    cargo.ends_with("mise/command-wrappers/bin/cargo")
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

#[cfg(test)]
mod tests {
    use super::{MIRROR_MARKER, pick_real_cargo};
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A scratch tree that is removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("velnor-test-support-{}-{name}", std::process::id()));
            fs::remove_dir_all(&root).ok();
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        /// Writes an executable-looking `cargo` into `dir` and returns its directory.
        fn bin_dir(&self, dir: &str) -> PathBuf {
            let dir = self.0.join(dir);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("cargo"), "#!/bin/sh\n").unwrap();
            dir
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    /// The `cargo` each PATH entry would offer, in PATH order.
    fn on_path(dirs: &[&Path]) -> Vec<PathBuf> {
        dirs.iter().map(|dir| dir.join("cargo")).collect()
    }

    #[test]
    fn the_mise_command_wrapper_beats_rustup_and_skips_the_mirror() {
        let scratch = Scratch::new("mise-wins");
        let mirror = scratch.bin_dir("mirror");
        fs::write(mirror.join("config.toml"), format!("{MIRROR_MARKER}\n")).unwrap();
        let mise = scratch.bin_dir("mise/command-wrappers/bin");
        let rustup = scratch.bin_dir(".cargo/bin");

        let picked = pick_real_cargo(on_path(&[&mirror, &mise, &rustup]));
        assert_eq!(picked, Some(mise.join("cargo")));

        let picked = pick_real_cargo(on_path(&[&mirror, &rustup, &mise]));
        assert_eq!(picked, Some(mise.join("cargo")));
    }

    #[test]
    fn without_mise_the_rustup_cargo_is_the_real_one() {
        let scratch = Scratch::new("no-mise");
        let mirror = scratch.bin_dir("mirror");
        fs::write(mirror.join("config.toml"), format!("{MIRROR_MARKER}\n")).unwrap();
        let rustup = scratch.bin_dir(".cargo/bin");

        assert_eq!(
            pick_real_cargo(on_path(&[&mirror, &rustup])),
            Some(rustup.join("cargo"))
        );
    }

    #[test]
    fn only_the_mirror_on_the_path_is_an_error() {
        let scratch = Scratch::new("only-mirror");
        let mirror = scratch.bin_dir("mirror");
        fs::write(mirror.join("config.toml"), format!("{MIRROR_MARKER}\n")).unwrap();

        assert_eq!(pick_real_cargo(on_path(&[&mirror])), None);
    }
}
