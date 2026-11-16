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
    let version = env!("CARGO_PKG_VERSION");
    let tag = format!("rust-ci-v{version}");
    assert_eq!(
        check_tag(version, &tag),
        Ok(format!("tag {tag} names version {version}"))
    );
    for tag in [
        "rust-ci-v0.0.0".to_owned(),
        format!("{tag}-rc1"),
        format!("{tag} "),
        format!("v{version}"),
        version.to_owned(),
        String::new(),
    ] {
        assert_eq!(
            check_tag(version, &tag),
            Err(format!(
                "tag {tag} must be rust-ci-v{version}, because this program is version {version}"
            ))
        );
    }
}
