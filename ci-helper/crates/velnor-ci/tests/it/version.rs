// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::error::ErrorKind;
use velnor_ci::io::parse_cli;

#[test]
fn version_prints_the_version_and_a_commit() {
    let error = parse_cli(["velnor-ci", "--version"])
        .err()
        .unwrap_or_else(|| unreachable!("--version parsed as a command"));
    assert_eq!(error.kind(), ErrorKind::DisplayVersion);
    let text = error.to_string();
    let rest = text
        .strip_prefix(&format!("velnor-ci {} (", env!("CARGO_PKG_VERSION")))
        .unwrap_or_else(|| unreachable!("{text}"));
    let commit = rest
        .strip_suffix(")\n")
        .unwrap_or_else(|| unreachable!("{text}"));
    assert!(!commit.is_empty(), "{text}");
}
