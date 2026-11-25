// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use serde_json::{Value, json};
use velnor_ci::image_publish::checks::ci_path;
use velnor_ci::image_publish::gate::DEFAULT_WORKFLOW;

use super::fixture::{ARTIFACTS, Candidate, PRODUCER_WORKFLOW};
use super::gates::gate;
use super::publish::PASS;
use super::registry::Fake;

const RUN: &str = "Candidate run identity is different from the producer run identity.";
const ARTIFACT: &str = "Candidate artifact ID, expiry, or workflow run is incorrect.";
const ONE: &str = "This workflow run must have one candidate artifact.";
const NO_SUCCESS: &str = "The latest canonical main CI run has no `success` conclusion.";
const CI: &str = ".github/workflows/ci.yml";

#[test]
fn a_publisher_without_a_checkout_accepts_a_valid_producer_artifact() {
    let candidate = Candidate::new("no-checkout");
    candidate.drop_checkout();
    let (result, _) = candidate.publish(&Fake::default(), candidate.env());
    assert_eq!(result.as_deref(), Ok(PASS));
    assert!(candidate.path("evidence/publication.json").exists());
    let calls = candidate.gh_log();
    let signer = format!("--signer-workflow {PRODUCER_WORKFLOW} ");
    assert_eq!(calls.matches(&signer).count(), 2, "{calls}");
    assert!(calls.contains("actions/runs/50/artifacts"), "{calls}");
    assert!(!calls.contains("actions/runs/77"), "{calls}");
}

fn rewrite(candidate: &Candidate, field: &str, value: &Value) {
    let mut record = candidate.record.clone();
    record["workflow"][field] = value.clone();
    fs::write(candidate.path("out/candidate.json"), record.to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
}

macro_rules! refused {
    ($name:ident, $label:expr, $change:expr, $message:expr) => {
        #[test]
        fn $name() {
            gate($label, $change, $message);
        }
    };
}

fn publisher_ref() -> String {
    format!("{DEFAULT_WORKFLOW}@refs/heads/main")
}

fn dev_ref() -> String {
    format!("{PRODUCER_WORKFLOW}@refs/heads/dev")
}

refused!(
    a_record_of_the_publishing_workflow_is_refused,
    "record-workflow",
    |c| rewrite(c, "workflow_ref", &json!(publisher_ref())),
    RUN
);
refused!(
    a_record_of_another_ref_is_refused,
    "record-ref",
    |c| rewrite(c, "workflow_ref", &json!(dev_ref())),
    RUN
);
refused!(
    a_record_of_another_event_is_refused,
    "record-event",
    |c| rewrite(c, "event", &json!("workflow_dispatch")),
    RUN
);
refused!(
    a_record_of_the_publishing_run_is_refused,
    "record-run",
    |c| rewrite(c, "run_id", &json!(77)),
    RUN
);
refused!(
    a_record_of_another_attempt_is_refused,
    "record-attempt",
    |c| rewrite(c, "run_attempt", &json!(2)),
    RUN
);

fn artifact(candidate: &Candidate, field: &str, value: &Value) {
    let mut listed = json!({"name": "velnor-job-image-candidate-1", "id": 99, "expired": false,
        "workflow_run": {"id": 50, "head_sha": candidate.head, "head_branch": "main"}});
    match field.split_once('.') {
        Some((outer, inner)) => listed[outer][inner] = value.clone(),
        None => listed[field] = value.clone(),
    }
    candidate.api(ARTIFACTS, &json!({"artifacts": [listed]}));
}

refused!(
    an_artifact_of_another_run_is_refused,
    "artifact-run",
    |c| artifact(c, "workflow_run.id", &json!(77)),
    ARTIFACT
);
refused!(
    an_artifact_of_another_branch_is_refused,
    "artifact-branch",
    |c| artifact(c, "workflow_run.head_branch", &json!("dev")),
    ARTIFACT
);
refused!(
    an_expired_artifact_is_refused,
    "artifact-expired",
    |c| artifact(c, "expired", &json!(true)),
    ARTIFACT
);
refused!(
    an_artifact_of_another_attempt_is_refused,
    "artifact-attempt",
    |c| artifact(c, "name", &json!("velnor-job-image-candidate-2")),
    ONE
);

fn ci_run(candidate: &Candidate, status: &str, conclusion: &str, path: &str) {
    let run = json!({"id": 50, "run_number": 5, "run_attempt": 1,
        "status": status, "conclusion": conclusion, "path": path, "event": "push",
        "head_branch": "main", "head_sha": candidate.head,
        "head_repository": {"full_name": "tailrocks/velnor-new"}});
    candidate.api(&ci_path(&candidate.head), &json!({"workflow_runs": [run]}));
}

refused!(
    a_failed_producer_run_is_refused,
    "ci-failure",
    |c| ci_run(c, "completed", "failure", CI),
    NO_SUCCESS
);
refused!(
    a_cancelled_producer_run_is_refused,
    "ci-cancelled",
    |c| ci_run(c, "completed", "cancelled", CI),
    NO_SUCCESS
);
refused!(
    an_unfinished_producer_run_is_refused,
    "ci-running",
    |c| ci_run(c, "in_progress", "", CI),
    NO_SUCCESS
);
refused!(
    a_producer_run_of_another_workflow_is_refused,
    "ci-workflow",
    |c| ci_run(c, "completed", "success", ".github/workflows/other.yml"),
    "No canonical main CI push run exists for this source."
);

#[test]
fn an_attestation_signed_by_the_publishing_trigger_is_refused() {
    gate(
        "publisher-signed",
        |c| {
            let signed = fs::read_to_string(c.path("attestation.json")).unwrap_or_default();
            let wrong = signed.replace("\"push\"", "\"workflow_dispatch\"");
            fs::write(c.path("attestation.json"), wrong).unwrap_or_else(|e| unreachable!("{e}"));
        },
        "Attestation certificate source or workflow identity is incorrect.",
    );
}

#[test]
fn the_evidence_names_the_producer_run_apart_from_the_publishing_run() {
    let candidate = Candidate::new("evidence");
    let (result, _) = candidate.publish(&Fake::default(), candidate.env());
    assert_eq!(result.as_deref(), Ok(PASS));
    let written = fs::read_to_string(candidate.path("evidence/publication.json"));
    let evidence: Value = serde_json::from_str(&written.unwrap_or_default()).unwrap_or_default();
    assert_eq!(evidence["ci"], json!({"run_id": 50, "run_attempt": 1}));
    assert_eq!(evidence["workflow"]["run_id"], 77);
}
