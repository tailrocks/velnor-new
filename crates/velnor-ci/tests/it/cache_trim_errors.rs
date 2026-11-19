// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::process::Command;

use crate::io::{Repo, serve_with_status};

#[test]
fn trim_list_errors_fail_the_job() {
    check_trim(&[], false);
}

#[test]
fn trim_delete_errors_fail_the_job() {
    check_trim(
        &[
            (
                LISTING,
                200,
                r#"{"total_count":1,"actions_caches":[{"id":1,"key":"base-1","created_at":"2026-11-17T00:00:00Z"}]}"#,
            ),
            ("/repos/owner/name/actions/caches/1", 500, ""),
        ],
        false,
    );
}

#[test]
fn trim_with_no_stale_entries_succeeds() {
    check_trim(
        &[(LISTING, 200, r#"{"total_count":0,"actions_caches":[]}"#)],
        true,
    );
}

const LISTING: &str =
    "/repos/owner/name/actions/caches?key=base-&ref=refs%2Fheads%2Fmain&per_page=100&page=1";

fn check_trim(responses: &[(&str, u16, &str)], empty: bool) {
    let responses = responses
        .iter()
        .map(|(path, code, body)| ((*path).to_owned(), (*code, (*body).to_owned())))
        .collect::<BTreeMap<_, _>>();
    let api = serve_with_status(responses);
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
