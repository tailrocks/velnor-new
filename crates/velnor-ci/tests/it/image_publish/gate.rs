// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use serde_json::{Value, json};
use velnor_ci::image_publish::gate::{
    DEFAULT_EVENT, DEFAULT_WORKFLOW, Identity, Policy, identity, record_matches,
};

pub(super) const SOURCE: &str = "1111111111111111111111111111111111111111";

pub(super) fn policy() -> Policy {
    Policy::new(DEFAULT_WORKFLOW, DEFAULT_EVENT).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(super) fn env() -> HashMap<String, String> {
    [
        ("GITHUB_REPOSITORY", "tailrocks/velnor-new"),
        ("GITHUB_REPOSITORY_ID", "1390620900"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_EVENT_NAME", "workflow_dispatch"),
        (
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml@refs/heads/main",
        ),
        ("GITHUB_SHA", SOURCE),
        ("GITHUB_WORKFLOW_SHA", SOURCE),
        ("GITHUB_RUN_ID", "77"),
        ("GITHUB_RUN_ATTEMPT", "2"),
        ("VELNOR_CANDIDATE_ARTIFACT_ID", "99"),
    ]
    .iter()
    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
    .collect()
}

fn verdict(change: impl Fn(&mut HashMap<String, String>)) -> String {
    let mut vars = env();
    change(&mut vars);
    identity(&vars, &policy()).map_or_else(|e| e.to_string(), |_| "ACCEPTED".to_owned())
}

#[test]
fn only_a_main_dispatch_of_the_publishing_workflow_passes_the_gate() {
    let accepted = identity(&env(), &policy()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        accepted,
        Identity {
            source: SOURCE.to_owned(),
            workflow_ref: policy().workflow_ref(),
            run_id: 77,
            run_attempt: 2,
            artifact_id: 99,
        }
    );
}

#[test]
fn a_pull_request_ref_is_refused_before_any_work() {
    for reference in [
        "refs/pull/7/merge",
        "refs/pull/7/head",
        "refs/tags/v1",
        "refs/heads/feature",
    ] {
        let verdict = verdict(|vars| {
            vars.insert("GITHUB_REF".to_owned(), reference.to_owned());
        });
        assert_eq!(
            verdict, "Publication is only for workflow_dispatch on refs/heads/main.",
            "{reference}"
        );
    }
    let event = verdict(|vars| {
        vars.insert("GITHUB_EVENT_NAME".to_owned(), "pull_request".to_owned());
    });
    assert_eq!(
        event,
        "Publication is only for workflow_dispatch on refs/heads/main."
    );
}

#[test]
fn a_policy_cannot_be_loosened_to_a_pull_request_or_a_foreign_workflow() {
    for (workflow, event) in [
        (DEFAULT_WORKFLOW, "pull_request"),
        (DEFAULT_WORKFLOW, "pull_request_target"),
        (DEFAULT_WORKFLOW, "schedule"),
        (DEFAULT_WORKFLOW, "workflow_run"),
        (DEFAULT_WORKFLOW, "Push"),
        ("other/repo/.github/workflows/x.yml", "push"),
        (
            "tailrocks/velnor-new/.github/workflows/x.yml@refs/heads/dev",
            "push",
        ),
        ("tailrocks/velnor-new/.github/workflows/", "push"),
        ("tailrocks/velnor-new/.github/workflows/a/b.yml", "push"),
        ("tailrocks/velnor-new/.github/workflows/x.sh", "push"),
    ] {
        assert!(Policy::new(workflow, event).is_err(), "{workflow} {event}");
    }
    let caller = Policy::new("tailrocks/velnor-new/.github/workflows/release.yml", "push")
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        caller.workflow_ref(),
        "tailrocks/velnor-new/.github/workflows/release.yml@refs/heads/main"
    );
}

#[test]
fn the_workflow_authority_and_repository_must_be_exact() {
    type Case = (&'static str, &'static str, &'static str);
    let cases: [Case; 6] = [
        (
            "GITHUB_REPOSITORY",
            "tailrocks/other",
            "Publication repository is incorrect.",
        ),
        (
            "GITHUB_REPOSITORY_ID",
            "5",
            "GitHub repository ID is incorrect.",
        ),
        (
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml@refs/heads/dev",
            "Publication workflow authority is incorrect.",
        ),
        (
            "GITHUB_WORKFLOW_SHA",
            "2222222222222222222222222222222222222222",
            "Workflow authority SHA is different from source SHA.",
        ),
        ("GITHUB_SHA", "abc", "Source SHA is incorrect."),
        ("GITHUB_RUN_ID", "0", "Publication run ID is incorrect."),
    ];
    for (name, value, message) in cases {
        let verdict = verdict(|vars| {
            vars.insert(name.to_owned(), value.to_owned());
        });
        assert_eq!(verdict, message, "{name}");
    }
    let missing = verdict(|vars| {
        vars.remove("VELNOR_CANDIDATE_ARTIFACT_ID");
    });
    assert_eq!(missing, "Required GitHub workflow identity is missing.");
}

fn record(change: impl Fn(&mut Value)) -> Value {
    let mut value = json!({
        "source": {"commit": SOURCE, "ref": "refs/heads/main"},
        "workflow": {
            "repository": "tailrocks/velnor-new",
            "repository_id": 1_390_620_900_u64,
            "event": "workflow_dispatch",
            "workflow_ref": policy().workflow_ref(),
            "authority_commit": SOURCE,
            "run_id": 77,
            "run_attempt": 2,
        },
    });
    change(&mut value);
    value
}

#[test]
fn a_record_from_another_run_or_source_is_not_the_candidate_of_this_run() {
    let accepted = identity(&env(), &policy()).unwrap_or_else(|e| unreachable!("{e}"));
    let check = |value: &Value| {
        let Value::Object(map) = value else {
            unreachable!("record is an object")
        };
        record_matches(map, &accepted, &policy()).map_err(|e| e.to_string())
    };
    assert_eq!(check(&record(|_| {})), Ok(()));
    let other_ref = record(|value| value["source"]["ref"] = json!("refs/heads/dev"));
    assert_eq!(
        check(&other_ref),
        Err("Candidate source is not the current main source.".to_owned())
    );
    for change in [
        |value: &mut Value| value["workflow"]["run_id"] = json!(78),
        |value: &mut Value| value["workflow"]["event"] = json!("push"),
        |value: &mut Value| value["workflow"]["repository_id"] = json!(5),
    ] {
        assert_eq!(
            check(&record(change)),
            Err(
                "Candidate run identity is different from the publication run identity.".to_owned()
            )
        );
    }
}
