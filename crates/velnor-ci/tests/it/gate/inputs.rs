// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::gate::{Verdict, release_gate};
use velnor_ci::github::{compare_path, rules_path};

use super::{GREEN, ONE_PASSED, assert_refused, both, check, required};
use crate::support::{FakeGitHub, repo};

#[test]
fn a_value_that_is_not_a_full_commit_sha_never_reaches_the_api() {
    let upper = GREEN.to_uppercase();
    for sha in ["", "main", "21ec8b9f", upper.as_str(), "../x"] {
        let said = release_gate(&FakeGitHub::default(), &repo(), sha, &[], None)
            .map_err(|e| e.to_string());
        assert_refused(&said, "not a full lowercase commit SHA");
    }
}

#[test]
fn the_default_branch_and_its_rules_are_whatever_the_repository_says() {
    let branch = "trunk/a b";
    let recorded = both()
        .edit("/repos/tailrocks/velnor-new", |body| {
            body["default_branch"] = json!(branch);
        })
        .set(
            rules_path(&repo(), branch, 1),
            json!([required(&[check("Required")])]),
        )
        .protection(branch, "off", &[], &json!([]))
        .set(
            compare_path(&repo(), branch, GREEN),
            json!({"status": "identical"}),
        )
        .green_run(|run| run["head_branch"] = json!(branch));
    let report = format!("commit {GREEN} is on {branch}\n{ONE_PASSED}ready=true\n");
    assert_eq!(
        recorded.gate(GREEN),
        Ok(Verdict {
            ready: true,
            report
        })
    );
    assert_eq!(
        rules_path(&repo(), "release/stable #1", 2),
        "/repos/tailrocks/velnor-new/rules/branches/release%2Fstable%20%231?per_page=100&page=2"
    );
    assert_eq!(
        compare_path(&repo(), "release/stable #1", GREEN),
        format!("/repos/tailrocks/velnor-new/compare/release%2Fstable%20%231...{GREEN}?per_page=1")
    );
}
