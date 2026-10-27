// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{POLICY, parse_policy, verify_transition};
use crate::homebrew::structural_cutover::protected_transition;
use velnor_actions_contract::{
    canonical_json_bytes, homebrew_adoption::HomebrewAdoptionTransition,
};

fn transition() -> HomebrewAdoptionTransition {
    protected_transition()
        .expect("protected transition parses")
        .transition()
        .clone()
}

fn policy_value(transition: &HomebrewAdoptionTransition) -> serde_json::Value {
    serde_json::json!({
        "author": {"id": 139_017, "login": "donbeave", "type": "User"},
        "schema": "velnor.homebrew-cutover-author-policy.v1",
        "tap_repository": transition.tap_repository,
        "tap_repository_id": transition.tap_repository_id,
        "transition_digest": transition.digest().expect("transition digest"),
        "transition_id": transition.transition_id,
    })
}

#[test]
fn compiled_policy_matches_the_protected_transition() {
    let transition = transition();
    let author = verify_transition(&transition).expect("compiled policy validates");
    assert_eq!(author.id, 139_017);
    assert_eq!(author.login, "donbeave");
    assert_eq!(author.account_type, "User");
}

#[test]
fn policy_rejects_binding_changes_and_noncanonical_documents() {
    let transition = transition();
    let mut cases = Vec::new();
    for (field, replacement) in [
        (
            "transition_digest",
            serde_json::json!("b3-".to_owned() + &"0".repeat(64)),
        ),
        ("transition_id", serde_json::json!("other-transition")),
        ("tap_repository", serde_json::json!("other/project")),
        ("schema", serde_json::json!("unsupported")),
    ] {
        let mut value = policy_value(&transition);
        value[field] = replacement;
        cases.push((
            field,
            canonical_json_bytes(&value).expect("canonical policy"),
        ));
    }
    let mut unknown = policy_value(&transition);
    unknown["unexpected"] = serde_json::json!(true);
    cases.push((
        "unknown field",
        canonical_json_bytes(&unknown).expect("canonical policy"),
    ));
    let mut nested_unknown = policy_value(&transition);
    nested_unknown["author"]["unexpected"] = serde_json::json!(true);
    cases.push((
        "unknown author field",
        canonical_json_bytes(&nested_unknown).expect("canonical policy"),
    ));
    let canonical = canonical_json_bytes(&policy_value(&transition)).expect("canonical policy");
    cases.push(("noncanonical", [canonical.as_slice(), b"\n"].concat()));
    cases.push((
        "duplicate",
        [b"{\"schema\":\"duplicate\",".as_slice(), &canonical[1..]].concat(),
    ));
    cases.push(("oversize", vec![b' '; POLICY.len().max(4097)]));
    for (case, bytes) in cases {
        assert!(parse_policy(&bytes, &transition).is_err(), "{case}");
    }
}

#[test]
fn policy_rejects_mismatched_transition_identity() {
    let policy = POLICY;
    let mut transition = transition();
    transition.tap_repository_id += 1;
    assert!(parse_policy(policy, &transition).is_err());
}

#[test]
fn rest_user_dto_requires_positive_id_safe_login_and_literal_user_type() {
    let transition = transition();
    let positive: crate::github_landed_pull::GitHubRestUser = serde_json::from_value(
        serde_json::json!({"id": 139_017, "login": "donbeave", "type": "User", "avatar_url": "https://example.invalid"}),
    )
    .expect("REST User DTO parses");
    assert!(positive.is_user());
    let expected = verify_transition(&transition).expect("compiled policy validates");
    super::require_author(&expected, &positive).expect("authorized author matches");

    for value in [
        serde_json::json!({"id": 0, "login": "donbeave", "type": "User"}),
        serde_json::json!({"id": 139_018, "login": "donbeave", "type": "User"}),
        serde_json::json!({"id": 139_017, "login": "other", "type": "User"}),
        serde_json::json!({"id": 139_017, "login": "donbeave", "type": "Bot"}),
        serde_json::json!({"id": 139_017, "login": null, "type": "User"}),
        serde_json::json!({"id": 139_017, "type": "User"}),
        serde_json::json!({"id": null, "login": "donbeave", "type": "User"}),
    ] {
        if let Ok(user) = serde_json::from_value::<crate::github_landed_pull::GitHubRestUser>(value)
        {
            assert!(super::require_author(&expected, &user).is_err());
        }
    }
}
