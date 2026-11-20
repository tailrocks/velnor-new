// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use velnor_ci::artifact_stage::Skip;

use crate::artifact_stage::{copied, entries, link, site, skipped, staged};
#[test]
fn a_wildcard_in_the_first_part_stages_only_the_matching_entries() {
    let site = site("stage-top-wildcard");
    site.repo.write_unstaged("one.xml", "1\n");
    site.repo.write_unstaged("reports-a/junit.xml", "2\n");
    site.repo.write_unstaged("reports-a/other.xml", "no\n");
    site.repo.write_unstaged("src/deep/three.xml", "no\n");
    site.repo.write_unstaged("target/big/four.xml", "no\n");
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("src/unrelated.xml"),
    );
    let result = staged(&site, &["*.xml", "reports*/junit.xml"]);
    assert_eq!(copied(&result), ["one.xml", "reports-a/junit.xml"]);
    assert_eq!(skipped(&result), Vec::<(String, Skip)>::new());
}

#[test]
fn a_path_that_starts_with_a_hyphen_is_a_path_not_an_option() {
    let site = site("stage-hyphen");
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("--help"),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env("GITHUB_WORKSPACE", site.repo.root())
        .args(["artifacts", "stage", "--into"])
        .arg(site.staging.root())
        .args(["--path", "--help"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("::warning::") && stdout.contains("--help"),
        "{stdout}"
    );
    assert_eq!(entries(site.staging.root()), Vec::<(String, bool)>::new());
}
