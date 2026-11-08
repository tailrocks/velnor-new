// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{ImageCandidateWorkflow, native_image_qualification_identity};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const QUEUE: &str =
    "refs/heads/gh-readonly-queue/main/pr-42-0123456789abcdef0123456789abcdef01234567";
const MAIN_WORKFLOW: &str = "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main";

fn workflow(reference: &str) -> ImageCandidateWorkflow {
    ImageCandidateWorkflow {
        repository: "tailrocks/velnor-new".into(),
        repository_id: 1_390_620_900,
        event: "merge_group".into(),
        workflow_ref: format!("tailrocks/velnor-new/.github/workflows/ci.yml@{reference}"),
        authority_commit: SHA.into(),
        run_id: 1,
        run_attempt: 1,
    }
}

#[test]
fn merge_group_selects_the_real_qualification() {
    assert!(super::qualification_runs("merge_group", QUEUE));
    assert!(super::qualification_runs(
        "pull_request",
        "refs/pull/42/merge"
    ));
    assert!(super::qualification_runs("push", "refs/heads/main"));
    assert!(!super::qualification_runs("push", "refs/heads/feature"));
    assert!(!super::qualification_runs("schedule", "refs/heads/main"));
}

#[test]
fn merge_group_identity_binds_queue_and_workflow() {
    for reference in [QUEUE, "refs/heads/main"] {
        assert!(native_image_qualification_identity(
            QUEUE,
            SHA,
            &workflow(reference)
        ));
    }
    assert!(native_image_qualification_identity(
        "refs/heads/gh-readonly-queue/main/pr-42-a",
        SHA,
        &workflow("refs/heads/main")
    ));
    for reference in [
        "refs/heads/gh-readonly-queue/other/pr-42-0123456789abcdef0123456789abcdef01234567",
        "refs/heads/gh-readonly-queue/main/pr-0-01234567",
        "refs/heads/gh-readonly-queue/main/pr-01-01234567",
        "refs/heads/gh-readonly-queue/main/pr-42-",
        "refs/heads/gh-readonly-queue/main/pr-42-abcdefgh",
        "refs/heads/main",
    ] {
        assert!(
            !native_image_qualification_identity(reference, SHA, &workflow("refs/heads/main")),
            "{reference}"
        );
    }
    assert!(!native_image_qualification_identity(
        QUEUE,
        "invalid",
        &workflow("refs/heads/main")
    ));
    assert!(!native_image_qualification_identity(
        QUEUE,
        SHA,
        &workflow("refs/heads/gh-readonly-queue/main/pr-43-01234567")
    ));
    let mut invalid = workflow("refs/heads/main");
    invalid.workflow_ref = MAIN_WORKFLOW.replace("ci.yml", "other.yml");
    assert!(!native_image_qualification_identity(QUEUE, SHA, &invalid));
    invalid.workflow_ref = MAIN_WORKFLOW.into();
    invalid.run_id = 0;
    assert!(!native_image_qualification_identity(QUEUE, SHA, &invalid));
}
