// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::parse_lock;

const COMMIT: &str = "1390e6f6a4b64e1f33dbddfbeedcd097db32ec36";

fn lock(binary_table: &str) -> String {
    let sum = "a".repeat(64);
    format!(
        "version = \"0.1.111\"\n\
         commit = \"{COMMIT}\"\n\n\
         [velnor-ci]\n\
         linux-x64 = \"{sum}\"\n\
         linux-arm64 = \"{sum}\"\n\n\
         [velnor-gen]\n\
         linux-x64 = \"{sum}\"\n\
         linux-arm64 = \"{sum}\"\n\n\
         [velnor-gen-binary]\n\
         {binary_table}\n"
    )
}

fn parses(text: &str) -> bool {
    parse_lock(text.as_bytes()).is_ok()
}

#[test]
fn a_well_formed_generator_binary_table_parses() {
    let sum = "b".repeat(64);
    let text = lock(&format!("linux-x64 = \"{sum}\"\nlinux-arm64 = \"{sum}\""));
    assert!(parses(&text), "{text}");
}

#[test]
fn a_truncated_generator_binary_digest_is_refused() {
    let text = lock("linux-x64 = \"abc\"\nlinux-arm64 = \"abc\"");
    assert!(!parses(&text), "{text}");
}

#[test]
fn a_generator_binary_table_missing_a_required_platform_is_refused() {
    let sum = "b".repeat(64);
    let text = lock(&format!("linux-x64 = \"{sum}\""));
    assert!(!parses(&text), "{text}");
}

#[test]
fn a_generator_binary_table_with_an_unknown_platform_is_refused() {
    let sum = "b".repeat(64);
    let text = lock(&format!(
        "linux-x64 = \"{sum}\"\nlinux-arm64 = \"{sum}\"\nlinux-riscv64 = \"{sum}\""
    ));
    assert!(!parses(&text), "{text}");
}
