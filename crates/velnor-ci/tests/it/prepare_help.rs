// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

const DOCS: &str = include_str!("../../../../docs/rust-ci-cache-keys.md");

fn help() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["prepare", "--help"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).replace(['\n', ' '], " ")
}

#[test]
fn the_help_names_every_output_and_the_metadata_prerequisite() {
    let text = help();
    for needle in [
        "step output compat",
        "lock_<root>",
        ".velnor/rust-metadata.json",
        "RUSTUP_AUTO_INSTALL=0",
        "--lock-root",
        "--no-locks",
    ] {
        assert!(text.contains(needle), "{needle} missing from {text}");
    }
}

#[test]
fn the_docs_describe_what_the_help_documents() {
    for needle in [
        "lock_<root>",
        ".velnor/rust-metadata.json",
        "--lock-root",
        "--no-locks",
    ] {
        assert!(DOCS.contains(needle), "{needle} missing from the docs");
    }
}
