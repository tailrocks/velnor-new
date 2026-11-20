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
