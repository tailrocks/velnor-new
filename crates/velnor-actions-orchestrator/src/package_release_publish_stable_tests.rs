use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use velnor_actions_contract::publication_retry::PublicationIdentity;

fn identity() -> PublicationIdentity {
    PublicationIdentity {
        source_sha: "a".repeat(40),
        receipt_digest: "b".repeat(64),
        assets: BTreeMap::from([("app.tar.gz".into(), "c".repeat(64))]),
    }
}
fn release() -> Value {
    json!({"id":42,"tag_name":"v1.2.3","body":"owned-body","draft":true,"prerelease":false,"immutable":false,"assets":[]})
}
fn snapshot() -> Snapshot {
    Snapshot::new(&serde_json::from_value(release()).expect("release fixture"))
}
fn draft() -> Observation {
    Observation {
        state: PublicationState::Draft {
            source_sha: identity().source_sha,
            original_receipt_digest: Some(identity().receipt_digest),
            assets: BTreeMap::new(),
        },
        release: Some(snapshot()),
    }
}

#[test]
fn observations_are_consumed_before_wrong_action_or_validation_can_fail() {
    let mut pending = Some(draft());
    assert!(take_observation(&mut pending, &identity(), &Action::Publish).is_err());
    assert!(pending.is_none());
    assert!(take_observation(&mut pending, &identity(), &Action::Upload("app.tar.gz")).is_err());
    pending = Some(draft());
    let admitted = take_observation(&mut pending, &identity(), &Action::Upload("app.tar.gz"))
        .expect("exact action");
    assert_eq!(admitted.release.expect("fixed target").id, 42);
    assert!(pending.is_none());
    assert!(take_observation(&mut pending, &identity(), &Action::Upload("app.tar.gz")).is_err());
}

#[test]
fn snapshot_refresh_never_reselects_a_different_release_or_accepts_changed_body() {
    for (key, replacement) in [
        ("id", json!(43)),
        ("body", json!("other")),
        ("tag_name", json!("v9.9.9")),
        ("draft", json!(false)),
        ("immutable", json!(true)),
    ] {
        let mut changed = release();
        changed[key] = replacement;
        let (api, calls) = super::super::super::GitHubApi::with_fixture(
            "example/app",
            vec![
                ("repos/example/app/releases/42".into(), changed),
                (
                    "repos/example/app/releases/42/assets?per_page=100&page=1".into(),
                    json!([]),
                ),
            ],
        );
        assert!(refresh_snapshot(&api, &snapshot()).is_err(), "{key}");
        calls.assert_drained();
        assert!(
            !calls
                .observed()
                .iter()
                .any(|route| route.contains("releases?"))
        );
    }
}

#[test]
fn changed_asset_identity_blocks_the_consumed_target_before_any_write() {
    let (api, calls) = super::super::super::GitHubApi::with_fixture(
        "example/app",
        vec![
            ("repos/example/app/releases/42".into(), release()),
            (
                "repos/example/app/releases/42/assets?per_page=100&page=1".into(),
                json!([{"id":7,"name":"foreign.tar.gz","state":"uploaded","size":1,"digest":format!("sha256:{}", "d".repeat(64))}]),
            ),
        ],
    );
    assert!(refresh_snapshot(&api, &snapshot()).is_err());
    calls.assert_drained();
    assert_eq!(calls.observed().len(), 2);
}
