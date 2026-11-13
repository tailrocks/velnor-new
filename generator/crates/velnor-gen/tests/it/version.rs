// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::error::ErrorKind;
use velnor_gen::check_tag;
use velnor_gen::io::parse_cli;

#[test]
fn version_prints_the_version_and_a_commit() {
    let error = parse_cli(["velnor-gen", "--version"])
        .err()
        .unwrap_or_else(|| unreachable!("--version parsed as a command"));
    assert_eq!(error.kind(), ErrorKind::DisplayVersion);
    let text = error.to_string();
    let rest = text
        .strip_prefix(&format!("velnor-gen {} (", env!("CARGO_PKG_VERSION")))
        .unwrap_or_else(|| unreachable!("{text}"));
    let commit = rest
        .strip_suffix(")\n")
        .unwrap_or_else(|| unreachable!("{text}"));
    assert!(!commit.is_empty(), "{text}");
}

#[test]
fn only_the_tag_of_the_version_of_the_program_is_released() {
    assert_eq!(
        check_tag("0.1.79", "rust-ci-v0.1.79"),
        Ok("tag rust-ci-v0.1.79 names version 0.1.79".to_owned())
    );
    for tag in [
        "rust-ci-v0.1.80",
        "rust-ci-v0.1.7",
        "rust-ci-v0.1.79-rc1",
        "rust-ci-v0.1.79 ",
        "v0.1.79",
        "0.1.79",
        "",
    ] {
        assert_eq!(
            check_tag("0.1.79", tag),
            Err(format!(
                "tag {tag} must be rust-ci-v0.1.79, because this program is version 0.1.79"
            ))
        );
    }
}
