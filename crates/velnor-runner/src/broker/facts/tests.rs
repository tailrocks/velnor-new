// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::{PullRequestHead, TrustedEvent, TrustedFacts};
use crate::job_message::AgentJobRequestMessage;

const FIXTURE: &str = include_str!("../../../fixtures/exec/checkout_pull_request.json");

fn entry(key: &str, value: &Value) -> Value {
    json!({"k": key, "v": value})
}

fn string(value: &str) -> Value {
    json!({"t": 0, "s": value})
}

fn object(entries: &[Value]) -> Value {
    json!({"t": 2, "d": entries})
}

fn pull_request_head(
    base_id: Option<&str>,
    head_repo: Option<(&str, Option<u64>)>,
) -> Result<PullRequestHead> {
    let sha = "0123456789012345678901234567890123456789";
    let mut job: Value = serde_json::from_str(
        &FIXTURE
            .replace("@SHA@", sha)
            .replace("@SERVER_URL@", "https://github.com"),
    )?;
    let mut head = vec![entry("ref", &string("topic"))];
    if let Some((name, id)) = head_repo {
        let mut repo = vec![entry("full_name", &string(name))];
        if let Some(id) = id {
            repo.push(entry("id", &json!({"t": 4, "n": id})));
        }
        head.push(entry("repo", &object(&repo)));
    }
    let event = object(&[entry(
        "pull_request",
        &object(&[entry("head", &object(&head))]),
    )]);
    let github = job["ContextData"]["github"]["d"]
        .as_array_mut()
        .context("github context entries")?;
    github.push(entry("event", &event));
    if let Some(base_id) = base_id {
        github.retain(|item| item["k"] != "repository_id");
        github.push(entry("repository_id", &string(base_id)));
    }
    let message = AgentJobRequestMessage::parse_json(&job.to_string())?;
    match TrustedFacts::from_job_message(&message)?.event {
        TrustedEvent::PullRequest { head, .. } => Ok(head),
        other => anyhow::bail!("not a pull request event: {other:?}"),
    }
}

fn facts_for(event_name: &str, git_ref: &str) -> Result<TrustedFacts> {
    let mut job: Value = serde_json::from_str(
        &FIXTURE
            .replace("@SHA@", "0123456789012345678901234567890123456789")
            .replace("@SERVER_URL@", "https://github.com"),
    )?;
    let github = job["ContextData"]["github"]["d"]
        .as_array_mut()
        .context("github context entries")?;
    for entry in github.iter_mut() {
        match entry["k"].as_str() {
            Some("event_name") => entry["v"] = string(event_name),
            Some("ref") => entry["v"] = string(git_ref),
            _ => {}
        }
    }
    let message = AgentJobRequestMessage::parse_json(&job.to_string())?;
    Ok(TrustedFacts::from_job_message(&message)?)
}

#[test]
fn only_a_push_or_a_scheduled_or_dispatched_branch_run_may_write_stores() -> Result<()> {
    let cases = [
        ("push", "refs/heads/main", true),
        ("push", "refs/tags/v1", true),
        ("workflow_dispatch", "refs/heads/main", true),
        ("schedule", "refs/heads/main", true),
        ("workflow_dispatch", "refs/tags/v1", false),
        ("pull_request_target", "refs/heads/main", false),
        ("workflow_run", "refs/heads/main", false),
        ("repository_dispatch", "refs/heads/main", false),
        ("pull_request", "refs/pull/7/merge", false),
        (
            "merge_group",
            "refs/heads/gh-readonly-queue/main/pr-1",
            false,
        ),
        ("push", "refs/pull/7/merge", false),
    ];
    for (event_name, git_ref, expected) in cases {
        let facts = facts_for(event_name, git_ref)?;
        assert_eq!(facts.event_name, event_name);
        assert_eq!(
            facts.may_write_stores(),
            expected,
            "{event_name} on {git_ref}"
        );
    }
    Ok(())
}

#[test]
fn a_missing_or_zero_repository_id_has_no_facts() -> Result<()> {
    for id in [None, Some("0"), Some("x")] {
        let mut job: Value = serde_json::from_str(
            &FIXTURE
                .replace("@SHA@", "0123456789012345678901234567890123456789")
                .replace("@SERVER_URL@", "https://github.com"),
        )?;
        let github = job["ContextData"]["github"]["d"]
            .as_array_mut()
            .context("github context entries")?;
        github.retain(|entry| entry["k"] != "repository_id");
        if let Some(id) = id {
            github.push(entry("repository_id", &string(id)));
        }
        let message = AgentJobRequestMessage::parse_json(&job.to_string())?;
        assert!(TrustedFacts::from_job_message(&message).is_err(), "{id:?}");
    }
    let mut job: Value = serde_json::from_str(
        &FIXTURE
            .replace("@SHA@", "0123456789012345678901234567890123456789")
            .replace("@SERVER_URL@", "https://github.com"),
    )?;
    let github = job["ContextData"]["github"]["d"]
        .as_array_mut()
        .context("github context entries")?;
    github.retain(|entry| entry["k"] != "repository_id");
    github.push(entry("repository_id", &json!({"t": 4, "n": 42})));
    let message = AgentJobRequestMessage::parse_json(&job.to_string())?;
    assert_eq!(
        TrustedFacts::from_job_message(&message)?
            .repository_id
            .get(),
        42
    );
    let facts = br#"{"repository_id": 0, "repository": "a/b", "server_url": "https://github.com",
"default_branch": "main", "sha": "0123456789012345678901234567890123456789", "run_id": 1,
"run_attempt": 1, "event_name": "push", "event": {"kind": "branch", "branch": "main"}}"#;
    assert!(TrustedFacts::parse(facts).is_err());
    Ok(())
}

#[test]
fn a_job_message_without_an_event_name_has_no_facts() -> Result<()> {
    let mut job: Value = serde_json::from_str(
        &FIXTURE
            .replace("@SHA@", "0123456789012345678901234567890123456789")
            .replace("@SERVER_URL@", "https://github.com"),
    )?;
    let github = job["ContextData"]["github"]["d"]
        .as_array_mut()
        .context("github context entries")?;
    github.retain(|entry| entry["k"] != "event_name");
    let message = AgentJobRequestMessage::parse_json(&job.to_string())?;
    assert!(TrustedFacts::from_job_message(&message).is_err());
    Ok(())
}

#[test]
fn a_pull_request_head_is_the_base_repository_only_when_the_message_proves_it() -> Result<()> {
    use PullRequestHead::{Fork, SameRepository};
    let base = "tailrocks/velnor-new";
    let cases = [
        (None, None, Fork),
        (Some("1"), None, Fork),
        (None, Some((base, None)), SameRepository),
        (None, Some(("Tailrocks/Velnor-New", None)), SameRepository),
        (None, Some(("someone/velnor-new", None)), Fork),
        (Some("1"), Some((base, Some(1))), SameRepository),
        (Some("1"), Some((base, Some(2))), Fork),
        (
            Some("1"),
            Some(("someone/renamed", Some(1))),
            SameRepository,
        ),
        (Some("1"), Some((base, None)), SameRepository),
    ];
    for (base_id, head, expected) in cases {
        assert_eq!(
            pull_request_head(base_id, head)?,
            expected,
            "base id {base_id:?}, head {head:?}"
        );
    }
    Ok(())
}
