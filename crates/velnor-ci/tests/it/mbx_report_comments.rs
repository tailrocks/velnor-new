// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[test]
fn the_mbx_report_reader_source_keeps_no_explanatory_comments() {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/mbx_report.rs"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let comments: Vec<&str> = source
        .lines()
        .filter(|line| !line.starts_with("// SPDX-"))
        .filter(|line| line.contains("//"))
        .collect();
    assert!(comments.is_empty(), "{comments:#?}");
}
