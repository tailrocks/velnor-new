// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

const CONFIG: &str = include_str!("../../../../../.alint.yml");
const OVERSIZED: &str =
    include_str!("../../../../../fixtures/alint-negative/rust-max-lines/oversized.rs");
const SAMPLE: &str = r#"version: 1
rules:
  - id: rust-max-lines
    kind: file_max_lines
    paths:
      include:
        - "ci-helper/crates/*/src/**/*.rs"
        - "ci-helper/crates/*/tests/**/*.rs"
      exclude:
        - "**/fixtures/**"
        - "**/testdata/**"
    max_lines: 400
    level: error
    message: "Rust source exceeds 400 physical lines"
"#;

fn rust_max_lines_rule(config: &str) -> &str {
    let marker = "  - id: rust-max-lines\n";
    let start = config
        .find(marker)
        .unwrap_or_else(|| unreachable!("rust-max-lines rule"));
    let rest = &config[start + marker.len()..];
    let end = rest.find("\n  - id:").unwrap_or(rest.len());
    &rest[..end]
}

fn header_value<'text>(text: &'text str, key: &str) -> &'text str {
    let first = text
        .lines()
        .next()
        .unwrap_or_else(|| unreachable!("header"));
    let rest = first
        .strip_prefix("// ")
        .unwrap_or_else(|| unreachable!("header"));
    let (name, value) = rest
        .split_once(": ")
        .unwrap_or_else(|| unreachable!("header"));
    if name == key {
        value
    } else {
        unreachable!("{name}")
    }
}

fn write_file(path: &Path, text: &str) {
    let parent = path.parent().unwrap_or_else(|| unreachable!("parent"));
    fs::create_dir_all(parent).unwrap_or_else(|error| unreachable!("{error}"));
    fs::write(path, text).unwrap_or_else(|error| unreachable!("{error}"));
}

fn sample_tree() -> PathBuf {
    let root = std::env::temp_dir().join(format!("velnor-alint-cap-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap_or_else(|error| unreachable!("{error}"));
    }
    let filler = |count: usize| "// filler\n".repeat(count);
    write_file(&root.join(".alint.yml"), SAMPLE);
    write_file(
        &root.join("ci-helper/crates/demo/src/over.rs"),
        &filler(401),
    );
    write_file(&root.join("ci-helper/crates/demo/src/ok.rs"), &filler(400));
    write_file(
        &root.join("ci-helper/crates/demo/fixtures/skip.rs"),
        &filler(500),
    );
    write_file(
        &root.join("ci-helper/crates/demo/testdata/skip.rs"),
        &filler(500),
    );
    root
}

fn alint_installed() -> bool {
    match Command::new("alint").arg("--version").output() {
        Ok(output) => output.status.success(),
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => unreachable!("{error}"),
    }
}

fn alint_rejects_401_lines() {
    if !alint_installed() {
        return;
    }
    let root = sample_tree();
    let config = root.join(".alint.yml");
    let output = Command::new("alint")
        .arg("check")
        .arg("--config")
        .arg(&config)
        .arg("--fail-on-warning")
        .arg("--format")
        .arg("json")
        .arg("--color")
        .arg("never")
        .arg(&root)
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}{stdout}");
    assert!(
        stdout.contains("ci-helper/crates/demo/src/over.rs"),
        "{stdout}"
    );
    assert!(!stdout.contains("demo/src/ok.rs"), "{stdout}");
    assert!(!stdout.contains("fixtures/skip.rs"), "{stdout}");
    assert!(!stdout.contains("testdata/skip.rs"), "{stdout}");
    fs::remove_dir_all(&root).unwrap_or_else(|error| unreachable!("{error}"));
}

#[test]
fn a_401_line_file_fails_the_rust_max_lines_lint() {
    let rule = rust_max_lines_rule(CONFIG);
    for needle in [
        "\"ci-helper/crates/*/src/**/*.rs\"",
        "\"ci-helper/crates/*/tests/**/*.rs\"",
        "\"**/fixtures/**\"",
        "\"**/testdata/**\"",
        "max_lines: 400",
    ] {
        assert!(rule.contains(needle), "{needle} missing from {rule}");
    }
    let expected: usize = header_value(OVERSIZED, "expected-lines")
        .parse()
        .unwrap_or_else(|_| unreachable!("line-count header"));
    assert_eq!(OVERSIZED.lines().count(), expected);
    assert_eq!(expected, 401);
    assert_eq!("// filler\n".repeat(400).lines().count(), 400);
    alint_rejects_401_lines();
}
