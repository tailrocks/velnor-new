// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::image_publish::checks::{
    CiRun, artifact, attestation, attestation_args, ci_listing, ci_path, ci_run, main_tip,
    repository,
};
use velnor_ci::image_publish::evidence::{Published, encode};
use velnor_ci::image_publish::gate::identity;
use velnor_ci::image_publish::plan::blobs;

use super::gate::{SOURCE, env, policy};

const EVIDENCE: &str = include_str!("../data/image_publication_evidence.json");
const RECORD: &str = include_str!("../data/image_build_record.json");

fn run(number: u64, id: u64, status: &str, conclusion: &str) -> Value {
    json!({
        "id": id, "run_number": number, "run_attempt": 1, "status": status,
        "conclusion": conclusion, "path": ".github/workflows/ci.yml", "event": "push",
        "head_branch": "main", "head_sha": SOURCE,
        "head_repository": {"full_name": "tailrocks/velnor-new"},
    })
}

fn ci(runs: &[Value]) -> String {
    ci_run(runs, SOURCE).map_or_else(|e| e.to_string(), |found| format!("{found:?}"))
}

#[test]
fn the_latest_canonical_ci_push_run_must_have_succeeded() {
    assert_eq!(
        ci(&[run(5, 50, "completed", "success")]),
        format!(
            "{:?}",
            CiRun {
                run_id: 50,
                run_attempt: 1
            }
        )
    );
    assert_eq!(
        ci(&[
            run(5, 50, "completed", "success"),
            run(6, 60, "completed", "failure")
        ]),
        "The latest canonical main CI run has no `success` conclusion."
    );
    assert_eq!(
        ci(&[
            run(5, 50, "completed", "failure"),
            run(6, 60, "completed", "success")
        ]),
        format!(
            "{:?}",
            CiRun {
                run_id: 60,
                run_attempt: 1
            }
        )
    );
    assert_eq!(
        ci(&[run(5, 50, "in_progress", "")]),
        "The latest canonical main CI run has no `success` conclusion."
    );
    assert_eq!(
        ci(&[
            run(5, 50, "completed", "success"),
            run(5, 51, "completed", "success")
        ]),
        "The latest canonical main CI run has no `success` conclusion."
    );
}

#[test]
fn only_a_canonical_ci_run_for_this_source_counts() {
    let listing = ci_path(SOURCE);
    assert!(
        listing.contains("&branch=main&event=push&per_page=100"),
        "the 100-run page holds only canonical candidates: {listing}"
    );
    let mut pull = run(9, 90, "completed", "success");
    pull["event"] = json!("pull_request");
    let mut fork = run(9, 91, "completed", "success");
    fork["head_repository"]["full_name"] = json!("someone/velnor-new");
    let mut elsewhere = run(9, 92, "completed", "success");
    elsewhere["head_sha"] = json!("2".repeat(40));
    let mut renamed = run(9, 93, "completed", "success");
    renamed["path"] = json!(".github/workflows/other.yml");
    assert_eq!(
        ci(&[pull, fork, elsewhere, renamed]),
        "No canonical main CI push run exists for this source."
    );
    assert!(ci_listing(&json!({"workflow_runs": []})).is_ok_and(<[Value]>::is_empty));
    assert!(ci_listing(&json!({"runs": []})).is_err());
}

fn candidate_artifact() -> Value {
    json!({"artifacts": [
        {"name": "velnor-job-image-candidate-1", "id": 5, "expired": false},
        {"name": "velnor-job-image-candidate-2", "id": 99, "expired": false,
         "workflow_run": {"id": 77, "head_sha": SOURCE, "head_branch": "main"}},
    ]})
}

#[test]
fn exactly_one_unexpired_artifact_of_this_attempt_and_run_is_the_candidate() {
    let this = identity(&env(), &policy()).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(artifact(&candidate_artifact(), &this).is_ok());
    for change in [
        |value: &mut Value| value["artifacts"][1]["id"] = json!(100),
        |value: &mut Value| value["artifacts"][1]["expired"] = json!(true),
        |value: &mut Value| value["artifacts"][1]["workflow_run"]["id"] = json!(78),
        |value: &mut Value| value["artifacts"][1]["workflow_run"]["head_branch"] = json!("dev"),
        |value: &mut Value| value["artifacts"][1]["workflow_run"]["head_sha"] = json!("2"),
    ] {
        let mut listing = candidate_artifact();
        change(&mut listing);
        assert!(artifact(&listing, &this).is_err());
    }
    let mut twice = candidate_artifact();
    let copy = twice["artifacts"][1].clone();
    twice["artifacts"]
        .as_array_mut()
        .unwrap_or_else(|| unreachable!("array"))
        .push(copy);
    assert!(artifact(&twice, &this).is_err());
    assert!(artifact(&json!({"artifacts": []}), &this).is_err());
}

#[test]
fn the_repository_and_main_tip_must_match_the_gate() {
    assert!(
        repository(&json!({"full_name": "tailrocks/velnor-new", "id": 1_390_620_900_u64})).is_ok()
    );
    assert!(repository(&json!({"full_name": "tailrocks/velnor-new", "id": "1390620900"})).is_err());
    assert!(repository(&json!({"full_name": "x/y", "id": 1_390_620_900_u64})).is_err());
    assert!(main_tip(&json!({"sha": SOURCE}), SOURCE).is_ok());
    assert!(main_tip(&json!({"sha": "2".repeat(40)}), SOURCE).is_err());
}

fn certificate() -> Value {
    json!([{"verificationResult": {"signature": {"certificate": {
        "sourceRepositoryIdentifier": "1390620900",
        "githubWorkflowRepository": "tailrocks/velnor-new",
        "githubWorkflowSHA": SOURCE,
        "githubWorkflowRef": "refs/heads/main",
        "githubWorkflowTrigger": "workflow_dispatch",
        "sourceRepositoryRef": "refs/heads/main",
        "sourceRepositoryDigest": SOURCE,
    }}}}])
}

#[test]
fn an_attestation_certificate_must_name_this_repository_workflow_and_commit() {
    assert!(attestation(&certificate(), SOURCE, &policy()).is_ok());
    for (field, value) in [
        ("sourceRepositoryIdentifier", json!("5")),
        ("githubWorkflowRepository", json!("x/y")),
        ("githubWorkflowSHA", json!("2")),
        ("githubWorkflowRef", json!("refs/heads/dev")),
        ("githubWorkflowTrigger", json!("pull_request")),
        ("sourceRepositoryRef", json!("refs/pull/1/merge")),
        ("sourceRepositoryDigest", json!("2")),
    ] {
        let mut bad = certificate();
        bad[0]["verificationResult"]["signature"]["certificate"][field] = value;
        assert!(attestation(&bad, SOURCE, &policy()).is_err(), "{field}");
    }
    assert!(attestation(&json!([]), SOURCE, &policy()).is_err());
    assert!(attestation(&json!({}), SOURCE, &policy()).is_err());
    assert!(attestation(&json!([{}]), SOURCE, &policy()).is_err());
    let args = attestation_args("/work/a.tar", SOURCE, &policy());
    let at = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .map(|i| args[i + 1].clone())
    };
    assert_eq!(
        at("--signer-workflow").as_deref(),
        Some(policy().workflow())
    );
    assert_eq!(at("--source-ref").as_deref(), Some("refs/heads/main"));
    assert_eq!(at("--signer-digest").as_deref(), Some(SOURCE));
}

fn digest(number: u8) -> String {
    format!("sha256:{}", format!("{number:02x}").repeat(32))
}

#[test]
fn the_blobs_to_push_are_the_config_and_layers_once_each() {
    let manifest = json!({
        "config": {"digest": digest(1), "size": 10},
        "layers": [{"digest": digest(2), "size": 20}, {"digest": digest(2), "size": 20},
                   {"digest": digest(3), "size": 30}],
    });
    let planned = blobs(&manifest).unwrap_or_else(|e| unreachable!("{e}"));
    let seen: Vec<(String, u64)> = planned
        .iter()
        .map(|blob| (blob.digest.as_str().to_owned(), blob.size))
        .collect();
    assert_eq!(seen, [(digest(1), 10), (digest(2), 20), (digest(3), 30)]);
    let mut conflicting = manifest.clone();
    conflicting["layers"][1]["size"] = json!(21);
    assert!(blobs(&conflicting).is_err());
    for broken in [
        json!({"config": {"digest": "latest", "size": 1}, "layers": []}),
        json!({"config": {"digest": digest(1), "size": 0}, "layers": []}),
        json!({"layers": []}),
        json!({"config": {"digest": digest(1), "size": 1}}),
    ] {
        assert!(blobs(&broken).is_err());
    }
}

#[test]
fn the_publication_evidence_is_byte_identical_to_the_golden_evidence() {
    let record: Value = serde_json::from_str(&RECORD.replace("{COMMIT}", &"a".repeat(40)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let Value::Object(fields) = &record else {
        unreachable!("record is an object")
    };
    let mut vars = env();
    let source = "a".repeat(40);
    for name in ["GITHUB_SHA", "GITHUB_WORKFLOW_SHA"] {
        vars.insert(name.to_owned(), source.clone());
    }
    let this = identity(&vars, &policy()).unwrap_or_else(|e| unreachable!("{e}"));
    let bytes = encode(&Published {
        record: fields,
        record_sha256: &"b".repeat(64),
        identity: &this,
        ci: CiRun {
            run_id: 55,
            run_attempt: 1,
        },
        oci: &record["oci"],
    })
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(String::from_utf8_lossy(&bytes), EVIDENCE);
}
