//! Fixed release admission never scans unrelated release history.

use super::*;
use serde_json::json;

#[test]
fn fixed_release_uses_only_the_configured_id() {
    let body = json!({
        "id":42,"tag_name":"preview","draft":false,"prerelease":true,
        "immutable":false,"body":"journal","assets":[]
    });
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![("repos/example/app/releases/42".to_owned(), body)],
    );
    assert_eq!(fixed_release(&api, 42).expect("fixed release").id, 42);
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 1);
}

#[test]
fn fixed_release_rejects_substitution_and_lifecycle_changes() {
    for (field, value) in [
        ("id", json!(43)),
        ("tag_name", json!("v1.0.0")),
        ("draft", json!(true)),
        ("prerelease", json!(false)),
        ("immutable", json!(true)),
        ("immutable", json!(null)),
    ] {
        let mut body = json!({
            "id":42,"tag_name":"preview","draft":false,"prerelease":true,
            "immutable":false,"assets":[]
        });
        body[field] = value;
        let (api, fixture) = GitHubApi::with_fixture(
            "example/app",
            vec![("repos/example/app/releases/42".to_owned(), body)],
        );
        assert!(fixed_release(&api, 42).is_err(), "accepted changed {field}");
        fixture.assert_drained();
    }
}

fn comparison(before: &str) -> serde_json::Value {
    json!({"status":"ahead","ahead_by":1,"behind_by":0,
        "merge_base_commit":{"sha":before},"base_commit":{"sha":before}})
}

#[test]
fn ancestry_cache_retains_only_exact_successful_sha_pair_evidence() {
    let before = "a".repeat(40);
    let after = "b".repeat(40);
    let other = "c".repeat(40);
    let route =
        |base: &str| format!("repos/example/app/compare/{base}...{after}?per_page=1&page=2");
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (route(&before), json!({"invalid":"response"})),
            (route(&before), comparison(&before)),
            (route(&other), comparison(&other)),
        ],
    );
    let mut cache = authority::EvidenceCache::default();
    assert!(cache.ancestor(&api, &before, &after).is_err());
    assert!(
        cache
            .ancestor(&api, &before, &after)
            .expect("retry admitted")
    );
    assert!(
        cache
            .ancestor(&api, &before, &after)
            .expect("exact retained pair")
    );
    assert!(
        cache
            .ancestor(&api, &other, &after)
            .expect("different pair admitted")
    );
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 3);
}
