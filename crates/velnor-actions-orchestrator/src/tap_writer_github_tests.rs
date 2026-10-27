// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use super::{CommitCreateResponse, CommitResponse, RefObject};

const COMMIT: &str = include_str!("github_history_fixtures/rest-git-commit.json");
const REFERENCE: &str = include_str!("github_history_fixtures/rest-git-ref.json");
const HEAD_OID: &str = "ea9238fcf2892578ddd23d6f283038ea269bd14a";

#[test]
fn git_commit_dtos_accept_captured_tree_and_parents_without_ref_types() {
    let raw: Value = serde_json::from_str(COMMIT).expect("captured commit");
    assert!(raw["tree"].get("type").is_none());
    assert!(
        raw["parents"]
            .as_array()
            .expect("captured commit parents")
            .iter()
            .all(|parent| parent.get("type").is_none())
    );
    let read: CommitResponse = serde_json::from_str(COMMIT).expect("GET commit DTO");
    let created: CommitCreateResponse =
        serde_json::from_str(COMMIT).expect("commit creation DTO schema");
    assert_eq!(read.sha, HEAD_OID);
    assert_eq!(created.sha, HEAD_OID);
    assert_eq!(read.tree.sha, "9ee10a5d3f29859dbb11eaffc5256fb3599a5bd7");
    assert_eq!(created.tree.sha, read.tree.sha);
    let expected = [
        "d8db4ad97b06e5c2958f1398dfdcf9eae38901d6",
        "014a0f6071f2674635232880096d02b899651cd4",
    ];
    assert_eq!(
        read.parents
            .iter()
            .map(|parent| parent.sha.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        created
            .parents
            .iter()
            .map(|parent| parent.sha.as_str())
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn git_ref_objects_still_require_the_captured_object_type() {
    let raw: Value = serde_json::from_str(REFERENCE).expect("captured ref");
    let object = raw["object"].clone();
    let reference: RefObject = serde_json::from_value(object.clone()).expect("typed ref object");
    assert_eq!(reference.sha, HEAD_OID);
    assert_eq!(reference.object_type, "commit");
    let mut missing_type = object;
    missing_type
        .as_object_mut()
        .expect("captured ref object")
        .remove("type");
    assert!(
        serde_json::from_value::<RefObject>(missing_type).is_err(),
        "accepted untyped ref object"
    );
}
