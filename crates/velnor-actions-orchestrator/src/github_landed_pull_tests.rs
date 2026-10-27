// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::{RestLandedPull, RestPullRef, read_merge_oid};

const REPOSITORY: &str = "jackin-project/jackin";
const REPOSITORY_ID: u64 = 1_197_700_841;
const MERGE_OID: &str = "9b296fc56a61bbc28db76805fe2bac730bc040eb";
const REST: &str = include_str!("github_history_fixtures/rest-pull-1179.json");
const GRAPHQL: &str = include_str!("github_history_fixtures/graphql-pull-1179.json");

#[test]
fn landed_merge_oid_accepts_missing_or_matching_rest_merge_sha() {
    let raw: Value = serde_json::from_str(REST).expect("captured REST pull");
    assert!(raw.get("merge_commit_sha").is_none());
    for merge_sha in [None, Some(MERGE_OID)] {
        let rest = admitted_rest(&raw, merge_sha);
        let observed: Value = serde_json::from_str(GRAPHQL).expect("captured GraphQL pull");
        let oid = read_merge_oid(REPOSITORY_ID, REPOSITORY, &rest, |_, _| Ok(observed))
            .expect("matching landed pull identities rejected");
        assert_eq!(oid, MERGE_OID);
    }
}

#[test]
fn landed_merge_oid_rejects_inconsistent_rest_merge_sha() {
    let raw: Value = serde_json::from_str(REST).expect("captured REST pull");
    let rest = admitted_rest(&raw, Some("0000000000000000000000000000000000000000"));
    let observed: Value = serde_json::from_str(GRAPHQL).expect("captured GraphQL pull");
    let error = read_merge_oid(REPOSITORY_ID, REPOSITORY, &rest, |_, _| Ok(observed))
        .expect_err("accepted inconsistent REST merge SHA");
    assert!(
        error
            .to_string()
            .contains("tap_historical_merge_graphql_mismatch")
    );
}

#[test]
fn landed_merge_oid_rejects_substituted_or_incomplete_graphql_identity() {
    let raw: Value = serde_json::from_str(REST).expect("captured REST pull");
    let rest = admitted_rest(&raw, None);
    let substitutions = [
        (
            "repository ID",
            "/data/repository/fullDatabaseId",
            json!("1"),
        ),
        (
            "head repository ID",
            "/data/repository/pullRequest/headRepository/fullDatabaseId",
            json!("1"),
        ),
        (
            "head OID",
            "/data/repository/pullRequest/headRefOid",
            json!("0000000000000000000000000000000000000000"),
        ),
        (
            "base OID",
            "/data/repository/pullRequest/baseRefOid",
            json!("0000000000000000000000000000000000000000"),
        ),
        (
            "malformed BigInt",
            "/data/repository/fullDatabaseId",
            json!("not-an-integer"),
        ),
        (
            "overflow BigInt",
            "/data/repository/fullDatabaseId",
            json!("18446744073709551616"),
        ),
        (
            "partial errors",
            "/errors",
            json!([{"message":"fixture failure"}]),
        ),
        (
            "missing merge commit",
            "/data/repository/pullRequest/mergeCommit",
            Value::Null,
        ),
    ];
    for (label, pointer, replacement) in substitutions {
        let mut observed: Value = serde_json::from_str(GRAPHQL).expect("captured GraphQL pull");
        observed["errors"] = json!([]);
        *observed
            .pointer_mut(pointer)
            .expect("fixture substitution path") = replacement;
        let Err(error) = read_merge_oid(REPOSITORY_ID, REPOSITORY, &rest, |_, _| Ok(observed))
        else {
            panic!("accepted substituted {label}");
        };
        assert!(
            error
                .to_string()
                .contains("tap_historical_merge_graphql_mismatch"),
            "unexpected rejection for {label}: {error}"
        );
    }
}

fn admitted_rest<'a>(raw: &'a Value, merge_sha: Option<&'a str>) -> RestLandedPull<'a> {
    RestLandedPull {
        number: raw["number"].as_u64().expect("captured PR number"),
        state: text(raw, "/state"),
        merged: raw["merged"].as_bool().expect("captured merged state"),
        merged_at: text(raw, "/merged_at"),
        head: rest_ref(raw, "/head"),
        base: rest_ref(raw, "/base"),
        merge_sha,
    }
}

fn rest_ref<'a>(raw: &'a Value, pointer: &str) -> RestPullRef<'a> {
    let value = raw.pointer(pointer).expect("captured PR ref");
    RestPullRef {
        reference: text(value, "/ref"),
        sha: text(value, "/sha"),
        repository_id: value
            .pointer("/repo/id")
            .and_then(Value::as_u64)
            .expect("captured repository ID"),
        repository: text(value, "/repo/full_name"),
    }
}

fn text<'a>(raw: &'a Value, pointer: &str) -> &'a str {
    raw.pointer(pointer)
        .and_then(Value::as_str)
        .expect("captured string")
}
