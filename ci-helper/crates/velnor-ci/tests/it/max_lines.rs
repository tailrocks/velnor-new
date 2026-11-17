// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CONFIG: &str = include_str!("../../../../../.alint.yml");

fn rust_max_lines_rule(config: &str) -> &str {
    let marker = "  - id: rust-max-lines\n";
    let start = config
        .find(marker)
        .unwrap_or_else(|| unreachable!("rust-max-lines rule"));
    let rest = &config[start + marker.len()..];
    let end = rest.find("\n  - id:").unwrap_or(rest.len());
    &rest[..end]
}

fn configured_max_lines(rule: &str) -> usize {
    rule.lines()
        .find_map(|line| line.trim().strip_prefix("max_lines:"))
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or_else(|| unreachable!("max_lines"))
}

fn write_file(path: &Path, text: &str) {
    let parent = path.parent().unwrap_or_else(|| unreachable!("parent"));
    fs::create_dir_all(parent).unwrap_or_else(|error| unreachable!("{error}"));
    fs::write(path, text).unwrap_or_else(|error| unreachable!("{error}"));
}

fn sample_tree(rule: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("velnor-alint-cap-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap_or_else(|error| unreachable!("{error}"));
    }
    let filler = |count: usize| "// filler\n".repeat(count);
    let config = format!("version: 1\nrules:\n  - id: rust-max-lines\n{rule}");
    write_file(&root.join(".alint.yml"), &config);
    write_file(
        &root.join("ci-helper/crates/demo/src/over.rs"),
        &filler(401),
    );
    write_file(&root.join("ci-helper/crates/demo/src/ok.rs"), &filler(400));
    write_file(&root.join("ci-helper/crates/demo/build.rs"), &filler(401));
    write_file(
        &root.join("ci-helper/crates/demo/examples/demo.rs"),
        &filler(401),
    );
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

#[test]
fn a_401_line_file_fails_the_rust_max_lines_lint() {
    let rule = rust_max_lines_rule(CONFIG);
    assert_eq!(configured_max_lines(rule), 400);
    for glob in [
        "\"crates/velnor-actions-*/**/*.rs\"",
        "\"crates/velnor-runner/crates/*/**/*.rs\"",
        "\"ci-helper/crates/*/**/*.rs\"",
        "\"generator/crates/*/**/*.rs\"",
        "\"local/crates/*/**/*.rs\"",
    ] {
        assert!(rule.contains(glob), "{glob} missing from {rule}");
    }

    let installed = Command::new("alint")
        .arg("--version")
        .output()
        .unwrap_or_else(|error| panic!("alint must be installed to run this test: {error}"));
    assert!(installed.status.success(), "{installed:?}");

    let root = sample_tree(rule);
    let output = Command::new("alint")
        .arg("check")
        .arg("--config")
        .arg(root.join(".alint.yml"))
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
    assert!(
        stdout.contains("ci-helper/crates/demo/build.rs"),
        "{stdout}"
    );
    assert!(
        stdout.contains("ci-helper/crates/demo/examples/demo.rs"),
        "{stdout}"
    );
    assert!(!stdout.contains("demo/src/ok.rs"), "{stdout}");
    assert!(!stdout.contains("fixtures/skip.rs"), "{stdout}");
    assert!(!stdout.contains("testdata/skip.rs"), "{stdout}");
    fs::remove_dir_all(&root).unwrap_or_else(|error| unreachable!("{error}"));
}
