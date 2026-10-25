use super::admit_pull_request_context;
use crate::homebrew::authority::context::pull_request_context::RunnerFacts;
use velnor_actions_contract::WorkflowEvent;

fn pull() -> crate::package_release_publish::PublicPullRequestSnapshot {
    crate::package_release_publish::PublicPullRequestSnapshot {
        state: "open".to_owned(),
        draft: false,
        merged: false,
        base_repository: "owner/tap".to_owned(),
        base_repository_id: 7,
        base_ref: "main".to_owned(),
        base_sha: "a".repeat(40),
        head_repository: "owner/tap".to_owned(),
        head_repository_id: 7,
        head_ref: "change".to_owned(),
        head_sha: "b".repeat(40),
        merge_commit_sha: Some("c".repeat(40)),
    }
}

#[test]
fn pull_context_binds_head_merge_and_runner_sha_to_api_facts() {
    let accepted = pull();
    let runner_facts = runner("owner/tap", 7, "a", "b", "b");
    assert_eq!(
        admit(
            &accepted,
            WorkflowEvent::PullRequest,
            &"b".repeat(40),
            &"b".repeat(40),
            &runner_facts
        )
        .expect("authenticated PR"),
        ("a".repeat(40), "b".repeat(40))
    );

    let merge = "c".repeat(40);
    assert!(
        admit(
            &accepted,
            WorkflowEvent::PullRequest,
            &merge,
            &merge,
            &runner("owner/tap", 7, "a", "b", "c"),
        )
        .is_ok()
    );
    assert!(
        admit(
            &accepted,
            WorkflowEvent::PullRequest,
            &"b".repeat(40),
            &"b".repeat(40),
            &runner("owner/tap", 7, "a", "b", "c"),
        )
        .is_ok()
    );
    assert!(
        admit(
            &accepted,
            WorkflowEvent::PullRequest,
            &"d".repeat(40),
            &merge,
            &runner("owner/tap", 7, "a", "b", "c"),
        )
        .is_err()
    );
}

#[test]
fn pull_context_rejects_wrong_base_repository_or_changed_api_head() {
    let runner_facts = runner("owner/tap", 7, "a", "b", "b");
    let mut rejected = pull();
    rejected.base_ref = "release".to_owned();
    assert!(
        admit(
            &rejected,
            WorkflowEvent::PullRequest,
            &"b".repeat(40),
            &"b".repeat(40),
            &runner_facts
        )
        .is_err()
    );
    rejected = pull();
    rejected.head_repository = "fork/tap".to_owned();
    assert!(
        admit(
            &rejected,
            WorkflowEvent::PullRequest,
            &"b".repeat(40),
            &"b".repeat(40),
            &runner_facts
        )
        .is_err()
    );
    rejected = pull();
    rejected.draft = true;
    assert!(
        admit(
            &rejected,
            WorkflowEvent::PullRequest,
            &"b".repeat(40),
            &"b".repeat(40),
            &runner_facts
        )
        .is_err()
    );
    rejected = pull();
    rejected.head_sha = "c".repeat(40);
    assert!(
        admit(
            &rejected,
            WorkflowEvent::PullRequest,
            &"b".repeat(40),
            &"b".repeat(40),
            &runner_facts
        )
        .is_err()
    );
}

fn admit(
    pull: &crate::package_release_publish::PublicPullRequestSnapshot,
    event: WorkflowEvent,
    expected: &str,
    checkout: &str,
    runner: &RunnerFacts,
) -> Result<(String, String), crate::OrchestratorError> {
    admit_pull_request_context(pull, event, "owner/tap", 7, expected, checkout, runner)
}

#[test]
fn fork_pull_requires_distinct_authenticated_head_repository() {
    let mut fork = pull();
    fork.head_repository = "contributor/tap".to_owned();
    fork.head_repository_id = 9;
    let runner = runner("contributor/tap", 9, "a", "b", "b");
    assert!(
        admit_pull_request_context(
            &fork,
            WorkflowEvent::Fork,
            "owner/tap",
            7,
            &"b".repeat(40),
            &"b".repeat(40),
            &runner,
        )
        .is_ok()
    );
    assert!(
        admit_pull_request_context(
            &fork,
            WorkflowEvent::PullRequest,
            "owner/tap",
            7,
            &"b".repeat(40),
            &"b".repeat(40),
            &runner,
        )
        .is_err()
    );
    fork.head_repository_id = 7;
    assert!(
        admit_pull_request_context(
            &fork,
            WorkflowEvent::Fork,
            "owner/tap",
            7,
            &"b".repeat(40),
            &"b".repeat(40),
            &runner,
        )
        .is_err()
    );
}

fn runner(
    repository: &str,
    repository_id: u64,
    base: &str,
    head: &str,
    runner_sha: &str,
) -> RunnerFacts {
    RunnerFacts {
        sha: runner_sha.repeat(40),
        git_ref: "refs/pull/1/merge".to_owned(),
        payload_base_sha: base.repeat(40),
        payload_head_sha: head.repeat(40),
        payload_head_repository: repository.to_owned(),
        payload_head_repository_id: repository_id,
        payload_base_repository: "owner/tap".to_owned(),
        payload_base_repository_id: 7,
        payload_base_ref: "main".to_owned(),
        payload_head_ref: "change".to_owned(),
        payload_repository_id: 7,
    }
}
