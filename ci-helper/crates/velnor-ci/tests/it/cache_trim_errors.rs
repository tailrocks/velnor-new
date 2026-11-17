// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::process::Command;

use crate::io::{Repo, serve};

#[test]
fn trim_list_errors_fail_the_job() {
    check_trim(None, false);
}

#[test]
fn trim_delete_errors_fail_the_job() {
    check_trim(
        Some(r#"{"actions_caches":[{"id":1,"key":"base-1","created_at":"2026-11-17T00:00:00Z"}]}"#),
        false,
    );
}

#[test]
fn trim_with_no_stale_entries_succeeds() {
    check_trim(Some(r#"{"actions_caches":[]}"#), true);
}

fn check_trim(listing: Option<&str>, empty: bool) {
    let responses = listing.map(|body| (
            "/repos/owner/name/actions/caches?key=base-&ref=refs%2Fheads%2Fmain&per_page=100&page=1".to_owned(),
            body.to_owned(),
        )).into_iter().collect::<BTreeMap<_, _>>();
    let api = serve(responses);
    let repo = Repo::new("trim-http-errors");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["cache", "trim", "--prefix", "base-", "--keep", "0"])
        .env("GITHUB_TOKEN", "test-token")
        .env("GITHUB_REPOSITORY", "owner/name")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_API_URL", api)
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.success(), empty, "{output:?}");
    if !empty {
        assert!(!output.stderr.is_empty(), "{output:?}");
    }
}
