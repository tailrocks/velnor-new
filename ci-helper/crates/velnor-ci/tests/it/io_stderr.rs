// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use crate::io::Repo;

const FORGED: &str = "a\\n::error::FORGED\\n  ::stop-commands::x\\u2028::warning::LS";

fn forged_lines(stderr: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stderr)
        .split([
            '\n', '\r', '\u{2028}', '\u{2029}', '\u{85}', '\u{b}', '\u{c}',
        ])
        .filter(|line| line.trim().starts_with("::"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_cargo_error_that_echoes_hostile_text_prints_no_line_that_starts_a_workflow_command() {
    let repo = Repo::new("stderr-forged");
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(
        "Cargo.toml",
        &format!(
            "[package]\nname = \"p\"\nedition = \"2024\"\n\n[features]\n\"{FORGED}\" = [\"nofeat\"]\n"
        ),
    );
    repo.write("Cargo.lock", "version = 4\n");
    repo.write("src/lib.rs", "");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["inputs", "audit"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FORGED"), "{stderr}");
    assert!(stderr.starts_with("velnor-ci: "), "{stderr}");
    assert_eq!(
        forged_lines(&output.stderr),
        Vec::<String>::new(),
        "{stderr}"
    );
}

#[test]
fn a_command_line_error_that_echoes_a_hostile_argument_prints_no_workflow_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("x\n::error::FORGED\n  ::stop-commands::x\u{2028}::warning::LS")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FORGED"), "{stderr}");
    assert_eq!(
        forged_lines(&output.stderr),
        Vec::<String>::new(),
        "{stderr}"
    );
}

#[test]
fn a_command_line_error_that_echoes_a_legacy_command_prefix_prints_no_double_hash_marker() {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("x ##[error]FORGED")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FORGED"), "{stderr}");
    assert!(!stderr.contains("##["), "{stderr}");
}
