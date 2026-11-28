// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, UNIX_EPOCH};

use super::{audit, own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const TARGET: &str = "rust/target/debug";
const MANIFEST: &str = "rust/target/.velnor/restored-units.tsv";
const STALE: &str = "f00000000000000e";

fn with_stale_unit() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, TARGET, &[]);
    let stale = format!("{TARGET}/deps/app-{STALE}.d");
    unit(
        &repo,
        &stale,
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    unit(
        &repo,
        &format!("{TARGET}/.fingerprint/app-{STALE}/lib-app"),
        "",
    );
    repo.set_mtime(&stale, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    repo.set_mtime(
        "shared/greeting.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    repo
}

fn manifest(repo: &Repo, restored: &[&str]) {
    let lines: Vec<String> = restored
        .iter()
        .map(|hash| format!("restored {hash}"))
        .collect();
    unit(
        repo,
        MANIFEST,
        &format!("velnor-restored-units 3\n{}\n", lines.join("\n")),
    );
}

#[test]
fn a_stale_dep_info_of_a_restored_unit_does_not_refuse_the_audit() {
    let repo = with_stale_unit();
    manifest(&repo, &[STALE]);

    let output = audit(&repo, &["--write"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_stale_dep_info_the_manifest_does_not_list_refuses_the_audit() {
    let repo = with_stale_unit();
    manifest(&repo, &["f00000000000000d"]);

    let output = audit(&repo, &["--write"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("found stale dep-info"),
        "{output:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_manifest_of_an_older_format_does_not_excuse_a_stale_dep_info() {
    let repo = with_stale_unit();
    unit(
        &repo,
        MANIFEST,
        &format!("velnor-restored-units 2\n{STALE}\n"),
    );

    let output = audit(&repo, &["--write"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("found stale dep-info"),
        "{output:?}"
    );
}
