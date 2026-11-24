// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use super::Invocation;

fn location(line: &str, directory: &str) -> PathBuf {
    let command: Vec<String> = line.split(' ').map(str::to_owned).collect();
    Invocation::new(&command).location(Path::new(directory))
}

#[test]
fn a_chdir_global_option_moves_the_manifest_it_is_relative_to() {
    assert_eq!(
        location(
            "cargo +nightly -Z unstable-options -C sub build --manifest-path nested/Cargo.toml",
            "/repo"
        ),
        PathBuf::from("/repo/sub/nested"),
    );
}

#[test]
fn a_chdir_global_option_alone_moves_the_working_directory() {
    assert_eq!(
        location("cargo +nightly -Z unstable-options -C sub build", "/repo"),
        PathBuf::from("/repo/sub"),
    );
}

#[test]
fn an_attached_chdir_value_drops_one_optional_equals_sign() {
    for option in ["-Csub", "-C=sub"] {
        assert_eq!(
            location(
                &format!("cargo +nightly -Z unstable-options {option} build"),
                "/repo"
            ),
            PathBuf::from("/repo/sub"),
            "{option}",
        );
    }
}

#[test]
fn a_chdir_global_option_moves_every_subcommand_that_builds() {
    for subcommand in ["run", "r", "bench", "rustc"] {
        assert_eq!(
            location(
                &format!("cargo -Z unstable-options -C sub {subcommand}"),
                "/repo"
            ),
            PathBuf::from("/repo/sub"),
            "{subcommand}",
        );
        assert_eq!(
            location(
                &format!(
                    "cargo -Z unstable-options -C sub {subcommand} --manifest-path nested/Cargo.toml"
                ),
                "/repo"
            ),
            PathBuf::from("/repo/sub/nested"),
            "{subcommand}",
        );
    }
}
