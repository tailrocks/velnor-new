// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::event::{PullRequest, Push, Repository};
use velnor_local::scenario::Scenario;
use velnor_local::side::{Side, fork_heads, push_event, refuse_fork_branches, registered_on};
use velnor_local::standin::ClientCall;

const SHA: &str = "007c9fa8aac37d2aa7bc4a26876bf2654699f6bc";
const REPOSITORY: Repository<'static> = Repository {
    full_name: "tailrocks/velnor-lab",
    default_branch: "main",
};

fn pull(fork: bool) -> serde_json::Value {
    REPOSITORY.pull_request(&PullRequest {
        action: "synchronize",
        number: 2,
        base: ("main", SHA),
        head: ("patch", SHA),
        before: Some(SHA),
        merged: false,
        merge_commit: SHA,
        fork,
        closed_at: None,
        changed_files: 1,
    })
}

#[test]
fn only_a_pull_request_from_a_fork_is_untrusted_and_it_never_falls_back_to_the_trusted_side() {
    let push = REPOSITORY.push(&Push {
        branch: "patch",
        before: SHA,
        after: SHA,
    });
    assert_eq!(Side::of(&pull(true)), Side::Untrusted);
    assert_eq!(Side::of(&pull(false)), Side::Trusted);
    assert_eq!(Side::of(&push), Side::Trusted);
    assert_eq!(Side::of_role("fork-runner-1"), Side::Untrusted);
    assert_eq!(Side::of_role("runner-1"), Side::Trusted);
    assert_eq!(
        Side::Trusted.pick("trusted", Some("untrusted")),
        Ok("trusted")
    );
    assert_eq!(
        Side::Untrusted.pick("trusted", Some("untrusted")),
        Ok("untrusted")
    );
    assert!(Side::Untrusted.pick("trusted", None).is_err());
}

fn steps(json: &str) -> Vec<velnor_local::scenario::Step> {
    let text = format!(r#"{{"description": "x", "steps": [{json}]}}"#);
    Scenario::parse(text.as_bytes())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .steps
}

#[test]
fn a_fork_branch_is_never_dispatched_used_as_a_base_or_shared_with_the_repository() {
    let fork = r#"{"open_pull_request": {"head": "patch", "base": "main", "fork": true}}"#;
    let allowed = steps(&format!(
        r#"{fork}, {{"commit": {{"branch": "patch", "append": "a", "line": "b"}}}}, {{"push": "patch"}},
        {{"merge_group": "patch"}}, {{"merge_pull_request": "patch"}}, {{"workflow_dispatch": {{"branch": "main"}}}}"#
    ));
    assert_eq!(
        fork_heads(&allowed).into_iter().collect::<Vec<_>>(),
        ["patch"]
    );
    assert!(refuse_fork_branches(&allowed, "main").is_ok());
    for refused in [
        r#"{"workflow_dispatch": {"branch": "patch"}}"#,
        r#"{"open_pull_request": {"head": "other", "base": "patch"}}"#,
        r#"{"open_pull_request": {"head": "patch", "base": "main"}}"#,
    ] {
        let both = steps(&format!("{fork}, {refused}"));
        assert!(refuse_fork_branches(&both, "main").is_err(), "{refused}");
    }
    assert!(refuse_fork_branches(&steps(fork), "patch").is_err());
    assert!(
        fork_heads(&steps(
            r#"{"open_pull_request": {"head": "patch", "base": "main"}}"#
        ))
        .is_empty()
    );
}

#[test]
fn a_push_of_a_fork_branch_sends_no_push_event() {
    let forks = fork_heads(&steps(
        r#"{"open_pull_request": {"head": "patch", "base": "main", "fork": true}}"#,
    ));
    let push = |branch| Push {
        branch,
        before: SHA,
        after: SHA,
    };
    assert!(push_event(&REPOSITORY, &push("patch"), &forks).is_none());
    let sent = push_event(&REPOSITORY, &push("main"), &forks);
    assert_eq!(
        sent.as_ref().map(|payload| payload["ref"].clone()),
        Some(serde_json::json!("refs/heads/main"))
    );
}

#[test]
fn a_job_gets_no_secrets_variables_or_token_from_the_client() {
    let call = ClientCall {
        event: "pull_request",
        payload: "/events/2-pull_request.json",
        git_ref: "refs/pull/2/merge",
        sha: SHA,
        repository: "tailrocks/velnor-lab",
        workflow: Some(".github/workflows/ci.yml"),
    };
    let args = call.args();
    let flags: Vec<&str> = args
        .iter()
        .copied()
        .filter(|arg| arg.starts_with('-'))
        .collect();
    assert_eq!(
        flags,
        [
            "--server",
            "--event",
            "--payload",
            "--ref",
            "--sha",
            "--repository",
            "-W"
        ]
    );
    assert!(
        args.iter()
            .all(|arg| !arg.to_lowercase().contains("token")
                && !arg.to_lowercase().contains("secret")),
        "{args:?}"
    );
}

#[test]
fn a_fork_pull_request_is_registered_with_both_cache_servers() {
    assert_eq!(registered_on(true), [Side::Trusted, Side::Untrusted]);
    assert_eq!(registered_on(false), [Side::Trusted]);
}
