// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::validate_landed_commit;
use crate::tap_prepare::CommitResponse;

#[test]
fn cutover_commit_guard_binds_the_actual_commit_object() {
    let captured: CommitResponse = serde_json::from_slice(include_bytes!(
        "github_history_fixtures/cutover-tap-base-git-commit.json"
    ))
    .expect("captured Tap commit DTO");
    let commit = "921858f118256e16d3dc312c407814ab0fd8ef52";
    let tree = "ce52780173323bcfcecce152cc147d342d17b6cc";
    let base = "95f57691a34919b77822c9236383a9227c7a8cc2";
    let different = "e".repeat(40);
    validate_landed_commit(&captured, commit, base, tree).expect("exact captured identity");
    for (case, expected_commit, expected_base, expected_tree) in [
        ("wrong landed SHA", different.as_str(), base, tree),
        ("wrong candidate tree", commit, base, different.as_str()),
        ("wrong seed base", commit, different.as_str(), tree),
        ("invalid expected SHA", "not-a-sha", base, tree),
    ] {
        assert!(
            validate_landed_commit(&captured, expected_commit, expected_base, expected_tree)
                .is_err(),
            "{case}"
        );
    }
    let two_parents: CommitResponse = serde_json::from_slice(include_bytes!(
        "github_history_fixtures/rest-git-commit.json"
    ))
    .expect("captured two-parent PR head DTO");
    assert!(
        validate_landed_commit(
            &two_parents,
            &two_parents.sha,
            &two_parents.parents[0].sha,
            &two_parents.tree.sha,
        )
        .is_err(),
        "two-parent commit"
    );
}
