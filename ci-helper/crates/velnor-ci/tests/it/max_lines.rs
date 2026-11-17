// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const CONFIG: &str = include_str!("../../../../../.alint.yml");
const OVERSIZED: &str =
    include_str!("../../../../../fixtures/alint-negative/rust-max-lines/oversized.rs");

#[test]
fn rust_max_lines_rule_covers_rust_files_except_fixtures() {
    let marker = "  - id: rust-max-lines\n";
    let start = CONFIG
        .find(marker)
        .unwrap_or_else(|| unreachable!("rust-max-lines rule"));
    let rest = &CONFIG[start + marker.len()..];
    let end = rest.find("\n  - id:").unwrap_or(rest.len());
    let rule = &rest[..end];
    assert!(rule.contains("kind: file_max_lines"));
    assert!(rule.contains("max_lines: 400"));
    assert!(rule.contains("\"**/*.rs\""));
    assert!(rule.contains("\"**/fixtures/**\""));
    assert!(rule.contains("\"**/testdata/**\""));
    assert_eq!(OVERSIZED.lines().count(), 401);
}
