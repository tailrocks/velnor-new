// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;

use serde_json::{Value, json};
use velnor_ci::preview::authority::modes::Mode;
use velnor_ci::preview::json::{canonical, digest};

use crate::preview_authority_world::*;
use crate::preview_world::{R, SOURCE, linux_payload, sha};

#[test]
fn draft_creates_the_draft_release_and_asks_to_sign() {
    let s = Scenario::new("draft", 2);
    s.client.set(
        "/actions/runs/500/artifacts?per_page=100&page=1",
        json!({"total_count": 0, "artifacts": []}),
    );
    s.run(Mode::Draft).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        s.outputs(),
        "darwin_payload_artifact_id=103\nlinux_payload_artifact_id=102\nsource_artifact_id=101\nsign=true\n"
    );
    assert_eq!(
        s.client.writes.borrow().as_slice(),
        [("POST".to_owned(), "/releases".to_owned())]
    );
    let decision: Value = serde_json::from_slice(
        &fs::read(s.temp.join("source-preview-decision.json")).unwrap_or_default(),
    )
    .unwrap_or_default();
    assert_eq!(decision["release_id"], 9000);
    assert_eq!(decision["draft"]["run_attempt"], 2);
    assert_eq!(
        decision["targets"]["linux"]["payload"]["sha256"],
        digest(&linux_payload())
    );
}

#[test]
fn a_second_draft_in_the_same_run_does_not_overwrite_the_decision() {
    let s = Scenario::new("draft-twice", 2);
    s.client.set(
        "/actions/runs/500/artifacts?per_page=100&page=1",
        json!({"total_count": 0, "artifacts": []}),
    );
    s.run(Mode::Draft).unwrap_or_else(|e| unreachable!("{e}"));
    s.client.set(
        "/releases?per_page=100&page=1",
        json!([{"id": 9000, "tag_name": format!("sdk-preview-{SOURCE}")}]),
    );
    assert!(s.run(Mode::Draft).is_err());
}

fn signed_up(attempt: u64) -> Scenario {
    let s = Scenario::new(&format!("flow{attempt}"), 2);
    s.client.set(
        "/actions/runs/500/artifacts?per_page=100&page=1",
        json!({"total_count": 0, "artifacts": []}),
    );
    s.run(Mode::Draft).unwrap_or_else(|e| unreachable!("{e}"));
    let work = &s.base;
    fs::copy(s.temp.join("source-preview-decision.json"), {
        let target = work.join("source-preview-decision/source-preview-decision.json");
        write(&target, b"");
        target
    })
    .unwrap_or_else(|e| unreachable!("{e}"));
    let mut next = Scenario::new(&format!("flow{attempt}-prepare"), attempt);
    next.base.clone_from(&s.base);
    next.client = FakeClient {
        api: RefCell::new(s.client.api.borrow().clone()),
        zips: BTreeMap::new(),
        writes: RefCell::new(Vec::new()),
        deadline: s.client.deadline,
    };
    next
}

#[test]
fn prepare_writes_the_receipt_the_attest_job_signs() {
    let mut s = signed_up(3);
    s.vars.insert("SIGN".to_owned(), "true".to_owned());
    s.run(Mode::Prepare).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(s.outputs(), "sign=true\n");
    let bytes =
        fs::read(s.temp.join("SDK_PREVIEW_RECEIPT.json")).unwrap_or_else(|e| unreachable!("{e}"));
    let receipt: Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(canonical(&receipt), bytes);
    assert_eq!(receipt["signer"]["run_attempt"], 3);
    assert_eq!(receipt["draft"]["run_attempt"], 2);
    assert_eq!(receipt["source"]["run_attempt"], 1);
    assert_eq!(receipt["release_id"], 9000);
    assert_eq!(receipt["candidate_source_artifact_id"], 101);
    assert_eq!(
        receipt["targets"]["darwin"]["candidate_payload_artifact_id"],
        103
    );
    assert!(
        !s.temp
            .join("SDK_PREVIEW_RECEIPT.json.sigstore.jsonl")
            .exists()
    );
}

#[test]
fn publish_uploads_the_four_assets_and_publishes_an_immutable_release() {
    let mut s = signed_up(3);
    s.vars.insert("SIGN".to_owned(), "true".to_owned());
    s.run(Mode::Prepare).unwrap_or_else(|e| unreachable!("{e}"));
    let receipt = fs::read(s.temp.join("SDK_PREVIEW_RECEIPT.json")).unwrap_or_default();
    write(
        &s.base
            .join("source-preview-signed/SDK_PREVIEW_RECEIPT.json"),
        &receipt,
    );
    write(
        &s.base
            .join("source-preview-signed/SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"),
        b"{\"bundle\":1}\n",
    );
    s.vars
        .insert("PAIR_ORIGIN".to_owned(), "artifact".to_owned());
    s.vars.insert("PAIR_ID".to_owned(), "77".to_owned());
    s.run(Mode::Publish).unwrap_or_else(|e| unreachable!("{e}"));
    let writes = s.client.writes.borrow();
    let names: Vec<&str> = writes
        .iter()
        .filter(|(k, _)| k == "UPLOAD")
        .map(|(_, n)| n.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "SDK_PREVIEW_RECEIPT.json",
            "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl",
            DARWIN_NAME,
            LINUX_NAME
        ]
    );
    assert_eq!(
        writes.last(),
        Some(&("PATCH".to_owned(), "/releases/9000".to_owned()))
    );
    let release = s.client.api.borrow()[&format!("{R}/releases/9000")].clone();
    assert_eq!(release["draft"], false);
    assert_eq!(release["immutable"], true);
}

#[test]
fn publish_refuses_a_receipt_that_was_not_made_for_these_payloads() {
    let mut s = signed_up(3);
    s.vars.insert("SIGN".to_owned(), "true".to_owned());
    s.run(Mode::Prepare).unwrap_or_else(|e| unreachable!("{e}"));
    let receipt = String::from_utf8_lossy(
        &fs::read(s.temp.join("SDK_PREVIEW_RECEIPT.json")).unwrap_or_default(),
    )
    .replace(&digest(&linux_payload()), &sha(b"other"));
    write(
        &s.base
            .join("source-preview-signed/SDK_PREVIEW_RECEIPT.json"),
        receipt.as_bytes(),
    );
    write(
        &s.base
            .join("source-preview-signed/SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"),
        b"x\n",
    );
    s.vars
        .insert("PAIR_ORIGIN".to_owned(), "artifact".to_owned());
    s.vars.insert("PAIR_ID".to_owned(), "77".to_owned());
    assert!(s.run(Mode::Publish).is_err());
    assert!(s.client.writes.borrow().is_empty());
}

#[test]
fn every_broken_precondition_of_draft_is_refused() {
    type Break = fn(&Scenario);
    let cases: [(&str, Break); 8] = [
        ("tag is lightweight", |s| {
            s.client
                .update(&format!("/git/ref/tags/sdk-preview-{SOURCE}"), |v| {
                    v["object"]["type"] = json!("commit");
                });
        }),
        ("main is not ahead of the source", |s| {
            s.client
                .update(&format!("/compare/{SOURCE}...{MAIN}"), |v| {
                    v["status"] = json!("diverged");
                });
        }),
        ("admit job failed", |s| {
            s.client.update(
                "/actions/runs/500/attempts/1/jobs?per_page=100&page=1",
                |v| {
                    v["jobs"][0]["conclusion"] = json!("failure");
                },
            );
        }),
        ("admit run is another branch", |s| {
            s.client.update("/actions/runs/500/attempts/1", |v| {
                v["head_branch"] = json!("main");
            });
        }),
        ("a release of the tag exists with another marker", |s| {
            s.client.set(
                "/releases?per_page=100&page=1",
                json!([{"id": 9000, "tag_name": format!("sdk-preview-{SOURCE}"), "body": "x"}]),
            );
            s.client
                .set("/releases/9000", json!({"id": 9000, "body": "x"}));
        }),
        ("two releases carry the tag", |s| {
            let tag = format!("sdk-preview-{SOURCE}");
            s.client.set(
                "/releases?per_page=100&page=1",
                json!([{"id": 1, "tag_name": tag}, {"id": 2, "tag_name": tag}]),
            );
        }),
        ("the release list repeats an id", |s| {
            s.client.set(
                "/releases?per_page=100&page=1",
                json!([{"id": 1, "tag_name": "a"}, {"id": 1, "tag_name": "b"}]),
            );
        }),
        ("a payload is not an executable of the target", |s| {
            write(
                &s.base
                    .join(format!("source-preview-payload-darwin/{DARWIN_NAME}")),
                &linux_payload(),
            );
        }),
    ];
    for (name, change) in cases {
        let s = Scenario::new("refuse", 2);
        s.client.set(
            "/actions/runs/500/artifacts?per_page=100&page=1",
            json!({"total_count": 0, "artifacts": []}),
        );
        change(&s);
        assert!(s.run(Mode::Draft).is_err(), "accepted: {name}");
        assert!(
            s.client.writes.borrow().is_empty(),
            "wrote before refusing: {name}"
        );
    }
}

#[test]
fn the_environment_must_be_the_tag_push_of_the_source() {
    for (key, value) in [
        ("GITHUB_EVENT_NAME", "workflow_dispatch"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_REPOSITORY", "evil/repo"),
        ("GITHUB_RUN_ID", "501"),
        ("GITHUB_RUN_ATTEMPT", "0"),
        ("GITHUB_RUN_ATTEMPT", "52"),
        ("AUTHORITY_PATH", ".github/workflows/ci.yml"),
        ("AUTHORITY_SHA", "abc"),
        ("SOURCE_ARTIFACT_ID", "0"),
        ("DARWIN_PAYLOAD_ARTIFACT_ID", "x"),
    ] {
        let mut s = Scenario::new("env", 2);
        s.client.set(
            "/actions/runs/500/artifacts?per_page=100&page=1",
            json!({"total_count": 0, "artifacts": []}),
        );
        s.vars.insert(key.to_owned(), value.to_owned());
        assert!(s.run(Mode::Draft).is_err(), "accepted {key}={value}");
    }
}

#[test]
fn the_metadata_must_be_canonical_and_the_only_file() {
    let s = Scenario::new("metadata", 2);
    let path = s
        .base
        .join("source-preview-metadata/source-preview-metadata.json");
    write(
        &path,
        serde_json::to_string_pretty(&metadata())
            .unwrap_or_default()
            .as_bytes(),
    );
    assert!(s.run(Mode::Draft).is_err());
    let s = Scenario::new("metadata-extra", 2);
    write(&s.base.join("source-preview-metadata/extra"), b"x");
    assert!(s.run(Mode::Draft).is_err());
}

#[test]
fn an_unknown_mode_is_refused() {
    assert!(Mode::parse("sign").is_err());
    assert_eq!(Mode::parse("publish").ok(), Some(Mode::Publish));
}

#[test]
fn draft_reuses_the_candidate_of_an_existing_draft_release_only_when_every_artifact_matches() {
    let s = Scenario::new("existing", 2);
    s.draft_release();
    s.client.set(
        "/releases?per_page=100&page=1",
        json!([{"id": 9000, "tag_name": format!("sdk-preview-{SOURCE}")}]),
    );
    assert!(
        s.run(Mode::Draft).is_err(),
        "the candidate artifacts are not there"
    );
}
