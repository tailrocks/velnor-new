//! Exercise the actual fixed-route transport with no network or credentials.

use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use velnor_actions_contract::publication_retry::rolling::{
    RollingJournal, RollingOriginal, RollingPhase,
};

fn identity() -> PublicationIdentity {
    PublicationIdentity {
        source_sha: "a".repeat(40),
        receipt_digest: sha256(b"receipt"),
        assets: BTreeMap::from([("release.json".to_owned(), sha256(b"receipt"))]),
    }
}

fn envelope() -> Envelope {
    Envelope {
        schema: "velnor.rolling-preview-journal.v1".to_owned(),
        repository: "example/app".to_owned(),
        repository_id: 1,
        release_id: 42,
        tag: "preview".to_owned(),
        journal: RollingJournal {
            identity: identity(),
            original: RollingOriginal {
                run_id: 10,
                run_attempt: 1,
                artifact_id: 11,
                archive_sha256: "b".repeat(64),
            },
            phase: RollingPhase::Staging,
            staged: BTreeMap::new(),
            obsolete: Vec::new(),
        },
        predecessor: None,
    }
}

fn mutation(method: &str, route: &str, fields: Value) -> (String, Value) {
    let route = if method == "UPLOAD" {
        format!("UPLOAD https://uploads.github.com/repos/example/app/{route}")
    } else {
        format!("{method} repos/example/app/{route}")
    };
    let mut response = serde_json::Map::new();
    response.insert("expected_fields".to_owned(), fields);
    response.insert("response".to_owned(), json!({}));
    (route, Value::Object(response))
}

#[test]
fn actual_transport_has_fixed_release_stage_ids_receipt_and_nonforce_tag_routes() {
    let temp = tempfile::tempdir().expect("directory");
    std::fs::write(temp.path().join("release.json"), b"receipt").expect("original");
    let identity = identity();
    let mut snapshot = envelope();
    let begin = snapshot.body().expect("body");
    snapshot.journal.phase = RollingPhase::Replacing;
    let ready = snapshot.body().expect("body");
    snapshot.journal.phase = RollingPhase::Committed;
    let commit = snapshot.body().expect("body");
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            mutation("PATCH", "releases/42", json!({"body":begin})),
            mutation(
                "UPLOAD",
                &format!(
                    "releases/42/assets?name={}",
                    staged_name(&identity, "release.json")
                ),
                json!({"sha256":sha256(b"receipt"),"size":"7"}),
            ),
            mutation("PATCH", "releases/42", json!({"body":ready})),
            mutation("DELETE", "releases/assets/8", json!({})),
            mutation("DELETE", "releases/assets/9", json!({})),
            mutation(
                "PATCH",
                "git/refs/tags/preview",
                json!({"sha":"a".repeat(40),"force":"false"}),
            ),
            mutation(
                "PATCH",
                "releases/assets/10",
                json!({"name":"release.json"}),
            ),
            mutation("PATCH", "releases/42", json!({"body":commit})),
        ],
    );
    let mut envelope = envelope();
    write(&api, 42, &envelope).expect("durable begin");
    mutate(
        &api,
        42,
        &identity,
        temp.path(),
        RollingAction::UploadStage("release.json".to_owned()),
    )
    .expect("stage");
    envelope.journal.phase = RollingPhase::Replacing;
    write(&api, 42, &envelope).expect("frozen IDs marker");
    for action in [
        RollingAction::DeleteStarter(8),
        RollingAction::DeleteObsolete(9),
        RollingAction::AdvanceTag,
        RollingAction::Rename {
            id: 10,
            name: "release.json".to_owned(),
        },
    ] {
        mutate(&api, 42, &identity, temp.path(), action).expect("exact mutation");
    }
    envelope.journal.phase = RollingPhase::Committed;
    write(&api, 42, &envelope).expect("receipt committed");
    fixture.assert_drained();
    assert!(
        fixture
            .observed()
            .iter()
            .all(|route| !route.starts_with("POST ")
                && !route.contains("DELETE repos/example/app/releases/42"))
    );
}

#[test]
fn changed_original_or_unknown_asset_cannot_reach_transport() {
    let temp = tempfile::tempdir().expect("directory");
    std::fs::write(temp.path().join("release.json"), b"changed").expect("changed bytes");
    let (api, fixture) = GitHubApi::with_fixture("example/app", Vec::new());
    for action in [
        RollingAction::UploadStage("release.json".to_owned()),
        RollingAction::Rename {
            id: 10,
            name: "foreign".to_owned(),
        },
        RollingAction::DeleteObsolete(0),
        RollingAction::Reconcile,
        RollingAction::Commit,
    ] {
        assert!(mutate(&api, 42, &identity(), temp.path(), action).is_err());
    }
    assert!(write(&api, 99, &envelope()).is_err());
    fixture.assert_drained();
    assert_eq!(fixture.observed(), Vec::<String>::new());
}

#[test]
fn failed_mutation_is_not_automatically_repeated() {
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(
            "PATCH repos/example/app/git/refs/tags/preview".to_owned(),
            json!({"expected_fields":{"sha":"a".repeat(40),"force":"false"},"transport_error":true}),
        )],
    );
    assert!(
        mutate(
            &api,
            42,
            &identity(),
            Path::new("/unused"),
            RollingAction::AdvanceTag
        )
        .is_err()
    );
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 1);
}

#[test]
fn reported_digest_cannot_replace_actual_bytes_at_retry_acceptance() {
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(
            "DOWNLOAD repos/example/app/releases/assets/10".to_owned(),
            json!({"bytes":[116,97,109,112,101,114,101,100]}),
        )],
    );
    let release = serde_json::from_value(json!({"id":42,"tag_name":"preview","assets":[{
        "id":10,"name":"release.json","size":8,"digest":format!("sha256:{}",sha256(b"receipt"))}]}));
    let release: super::super::super::github::ReleaseResponse = release.expect("response");
    assert!(api.remote_assets(&release).is_err());
    fixture.assert_drained();
}
