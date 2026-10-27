// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{expected_author, read_detail};

const REPOSITORY: &str = "jackin-project/homebrew-tap";
const REPOSITORY_ID: u64 = 1_200_197_694;
const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MERGE: &str = "cccccccccccccccccccccccccccccccccccccccc";
const MERGED_AT: &str = "2026-10-27T00:00:00Z";

fn detail(user: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "number": 42,
        "state": "closed",
        "merged": true,
        "draft": false,
        "merged_at": MERGED_AT,
        "merge_commit_sha": MERGE,
        "user": user,
        "head": {
            "ref": "cutover",
            "sha": HEAD,
            "repo": {"id": REPOSITORY_ID, "full_name": REPOSITORY}
        },
        "base": {
            "ref": "main",
            "sha": BASE,
            "repo": {"id": REPOSITORY_ID, "full_name": REPOSITORY}
        }
    })
}

fn graphql() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "repository": {
                "fullDatabaseId": REPOSITORY_ID.to_string(),
                "nameWithOwner": REPOSITORY,
                "pullRequest": {
                    "number": 42,
                    "state": "MERGED",
                    "merged": true,
                    "mergedAt": MERGED_AT,
                    "headRefName": "cutover",
                    "headRefOid": HEAD,
                    "baseRefName": "main",
                    "baseRefOid": BASE,
                    "headRepository": {
                        "fullDatabaseId": REPOSITORY_ID.to_string(),
                        "nameWithOwner": REPOSITORY
                    },
                    "mergeCommit": {"oid": MERGE}
                }
            }
        }
    })
}

#[test]
fn cutover_history_reader_rejects_authenticated_author_drift() {
    let expected = expected_author().expect("compiled protected author policy");
    let admitted = read_detail(
        REPOSITORY,
        REPOSITORY_ID,
        42,
        &detail(&serde_json::json!({"id": 139_017, "login": "donbeave", "type": "User"})),
        &expected,
        |_, _| Ok(graphql()),
    )
    .expect("policy author history accepted");
    assert_eq!(admitted.author, expected);

    let rejected = read_detail(
        REPOSITORY,
        REPOSITORY_ID,
        42,
        &detail(&serde_json::json!({"id": 139_018, "login": "donbeave", "type": "User"})),
        &expected,
        |_, _| panic!("GraphQL must not run after REST author mismatch"),
    );
    assert!(
        rejected
            .expect_err("changed REST author must fail before merge identity read")
            .to_string()
            .contains("homebrew_cutover_author_mismatch")
    );
}
