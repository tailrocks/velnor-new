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

#[test]
fn a_file_with_more_than_one_hard_link_is_skipped_never_copied() {
    let site = site("stage-hard");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    site.repo.write_unstaged("logs/pair.log", "twin\n");
    std::fs::hard_link(
        site.repo.root().join("logs/pair.log"),
        site.repo.root().join("logs/twin.log"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::hard_link(
        site.outside.root().join("credentials"),
        site.repo.root().join("logs/leak.log"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let result = staged(&site, &["logs/"]);
    assert_eq!(copied(&result), ["logs/real.log"]);
    assert_eq!(
        skipped(&result),
        [
            ("logs/leak.log".to_owned(), Skip::HardLink),
            ("logs/pair.log".to_owned(), Skip::HardLink),
            ("logs/twin.log".to_owned(), Skip::HardLink),
        ]
    );
    assert_eq!(entries(site.staging.root()).len(), 2);
}

#[test]
fn a_file_name_that_actions_artifact_rejects_is_skipped_so_the_upload_still_succeeds() {
    let site = site("stage-names");
    site.repo.write_unstaged("logs/ok.log", "ok\n");
    let bad = [
        "a:b.log", "q\"x.log", "l<t.log", "g>t.log", "p|e.log", "st*r.log", "qu?.log", "c\rr.log",
        "n\nl.log",
    ];
    for name in bad {
        site.repo.write_unstaged(&format!("logs/{name}"), "x\n");
    }
    site.repo.write_unstaged("logs/dir:x/inner.log", "x\n");
    let result = staged(&site, &["logs/"]);
    assert_eq!(copied(&result), ["logs/ok.log"]);
    let skipped = skipped(&result);
    assert_eq!(skipped.len(), bad.len() + 1, "{skipped:?}");
    assert!(skipped.iter().all(|(_, skip)| *skip == Skip::Name));
    assert_eq!(entries(site.staging.root()).len(), 2);
}

#[cfg(target_os = "linux")]
#[test]
fn a_pattern_matches_case_sensitively_on_linux() {
    let site = site("stage-case");
    site.repo.write_unstaged("reports/JUnit.xml", "1\n");
    site.repo.write_unstaged("reports/junit.xml", "2\n");
    site.repo.write_unstaged("Docs/a.xml", "3\n");
    site.repo.write_unstaged("docs/a.xml", "4\n");
    let result = staged(&site, &["reports/JUnit.xml", "Docs/"]);
    assert_eq!(copied(&result), ["Docs/a.xml", "reports/JUnit.xml"]);
    assert_eq!(skipped(&result), Vec::<(String, Skip)>::new());
}
