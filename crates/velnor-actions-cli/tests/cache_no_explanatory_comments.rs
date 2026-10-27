// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const SOURCE: &str = include_str!("../src/dispatch/cache.rs");

#[test]
fn cache_rs_has_no_explanatory_comments() {
    let mut offenders = Vec::new();
    for (index, line) in SOURCE.lines().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with("// SPDX-") {
            continue;
        }
        offenders.push(index + 1);
    }
    assert!(
        offenders.is_empty(),
        "explanatory comments must not return to dispatch/cache.rs (lines {offenders:?})"
    );
}
