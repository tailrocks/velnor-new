// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::{FactsError, PullRequestHead, TrustedEvent, TrustedFacts};
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

const SHA: &str = "0123456789012345678901234567890123456789";
const BASE: &str = "tailrocks/velnor-new";
const SAME: Option<(&str, Option<u64>)> = Some((BASE, Some(1)));
const FORK: Option<(&str, Option<u64>)> = Some(("someone/velnor-new", Some(2)));

fn job() -> Result<Value> {
    Ok(serde_json::from_str(
        &FIXTURE
            .replace("@SHA@", SHA)
            .replace("@SERVER_URL@", "https://github.com"),
    )?)
}

fn github(job: &mut Value) -> Result<&mut Vec<Value>> {
    job["ContextData"]["github"]["d"]
        .as_array_mut()
        .context("github context entries")
}

fn parse(job: &Value) -> Result<TrustedFacts, FactsError> {
    let message = AgentJobRequestMessage::parse_json(&job.to_string()).map_err(|error| {
        FactsError::Invalid {
            field: "job",
            value: format!("{error:#}"),
        }
    })?;
    TrustedFacts::from_job_message(&message)
}

fn facts_for(
    event_name: &str,
    git_ref: &str,
    head_repo: Option<(&str, Option<u64>)>,
) -> Result<TrustedFacts> {
    let mut job = job()?;
    let github = github(&mut job)?;
    for entry in github.iter_mut() {
        match entry["k"].as_str() {
            Some("event_name") => entry["v"] = string(event_name),
            Some("ref") => entry["v"] = string(git_ref),
            _ => {}
        }
    }
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
    github.push(entry("event", &event));
    Ok(parse(&job)?)
}

fn pull_request_head(head_repo: Option<(&str, Option<u64>)>) -> Result<PullRequestHead> {
    match facts_for("pull_request", "refs/pull/7/merge", head_repo)?.event {
        TrustedEvent::PullRequest { head, .. } => Ok(head),
        other => anyhow::bail!("not a pull request event: {other:?}"),
    }
}

#[test]
fn only_events_that_write_their_own_scope_may_write_stores() -> Result<()> {
    let queue = "refs/heads/gh-readonly-queue/main/pr-1";
    let cases = [
        ("push", "refs/heads/main", None, true),
        ("push", "refs/tags/v1", None, true),
        ("push", queue, None, false),
        ("push", "refs/pull/7/merge", SAME, false),
        ("schedule", "refs/heads/main", None, true),
        ("workflow_dispatch", "refs/heads/main", None, true),
        ("workflow_dispatch", "refs/tags/v1", None, true),
        ("repository_dispatch", "refs/heads/main", None, true),
        ("release", "refs/tags/v1", None, true),
        ("pull_request", "refs/pull/7/merge", SAME, true),
        ("pull_request", "refs/pull/7/merge", FORK, false),
        ("pull_request", "refs/pull/7/merge", None, false),
        ("pull_request", "main", SAME, false),
        ("pull_request_target", "refs/heads/main", None, false),
        ("pull_request_target", "refs/pull/7/merge", SAME, false),
        ("issue_comment", "refs/heads/main", None, false),
        ("workflow_run", "refs/heads/main", None, false),
        ("merge_group", queue, None, false),
        ("delete", "refs/heads/main", None, false),
    ];
    for (event_name, git_ref, head, expected) in cases {
        let facts = facts_for(event_name, git_ref, head)?;
        assert_eq!(facts.event_name, event_name);
        assert_eq!(
            facts.may_write_stores(),
            expected,
            "{event_name} on {git_ref} with head {head:?}"
        );
    }
    Ok(())
}

#[test]
fn an_unqualified_ref_is_a_branch_as_actions_checkout_reads_it() -> Result<()> {
    let facts = facts_for("pull_request", "main", SAME)?;
    assert_eq!(
        facts.event,
        TrustedEvent::Branch {
            branch: "main".to_owned()
        }
    );
    assert_eq!(facts.git_ref(), "refs/heads/main");
    for git_ref in ["", "refs/other/x", "refs/heads"] {
        assert!(
            matches!(
                facts_for("push", git_ref, None),
                Err(error) if error.to_string().contains("ref")
            ),
            "{git_ref:?}"
        );
    }
    Ok(())
}

#[test]
fn a_missing_or_zero_repository_id_has_no_facts() -> Result<()> {
    for id in [None, Some("0"), Some("x")] {
        let mut job = job()?;
        let github = github(&mut job)?;
        github.retain(|entry| entry["k"] != "repository_id");
        if let Some(id) = id {
            github.push(entry("repository_id", &string(id)));
        }
        assert!(
            matches!(
                parse(&job),
                Err(FactsError::Missing {
                    field: "repository_id"
                })
            ),
            "{id:?}"
        );
    }
    let mut job = job()?;
    let github = github(&mut job)?;
    github.retain(|entry| entry["k"] != "repository_id");
    github.push(entry("repository_id", &json!({"t": 4, "n": 42})));
    assert_eq!(parse(&job)?.repository_id.get(), 42);
    let facts = br#"{"repository_id": 0, "repository": "a/b", "server_url": "https://github.com",
"default_branch": "main", "sha": "0123456789012345678901234567890123456789", "run_id": 1,
"run_attempt": 1, "event_name": "push", "event": {"kind": "branch", "branch": "main"}}"#;
    assert!(TrustedFacts::parse(facts).is_err());
    Ok(())
}

#[test]
fn a_job_message_without_an_event_name_has_no_facts() -> Result<()> {
    let mut job = job()?;
    github(&mut job)?.retain(|entry| entry["k"] != "event_name");
    assert!(matches!(
        parse(&job),
        Err(FactsError::Missing {
            field: "event_name"
        })
    ));
    Ok(())
}

#[test]
fn a_pull_request_head_is_the_base_repository_only_when_the_message_proves_it() -> Result<()> {
    use PullRequestHead::{Fork, SameRepository};
    let cases = [
        (None, Fork),
        (Some((BASE, None)), SameRepository),
        (Some(("Tailrocks/Velnor-New", None)), SameRepository),
        (Some(("someone/velnor-new", None)), Fork),
        (Some((BASE, Some(1))), SameRepository),
        (Some((BASE, Some(2))), Fork),
        (Some(("someone/renamed", Some(1))), SameRepository),
    ];
    for (head, expected) in cases {
        assert_eq!(pull_request_head(head)?, expected, "head {head:?}");
    }
    Ok(())
}

#[test]
fn a_server_url_with_credentials_is_refused_without_echoing_them() -> Result<()> {
    for server in [
        "https://user:secret@ghe.example",
        "https://secret@ghe.example",
        "http://:secret@ghe.example",
    ] {
        let mut job = job()?;
        let github = github(&mut job)?;
        github.retain(|entry| entry["k"] != "server_url");
        github.push(entry("server_url", &string(server)));
        let error = parse(&job).expect_err(server);
        assert!(
            matches!(
                error,
                FactsError::Invalid {
                    field: "server_url",
                    ..
                }
            ),
            "{error}"
        );
        assert!(!error.to_string().contains("secret"), "{error}");
    }
    Ok(())
}

#[test]
fn a_server_url_that_is_not_http_https_or_file_is_refused_without_echoing_it() -> Result<()> {
    for server in [
        "ssh://user:secret@ghe.example/path",
        "secret:topsecret@ghe.example",
        "ftp://user:secret@ghe.example",
        "not a url with secret",
    ] {
        let mut job = job()?;
        let github = github(&mut job)?;
        github.retain(|entry| entry["k"] != "server_url");
        github.push(entry("server_url", &string(server)));
        let error = parse(&job).expect_err(server);
        assert!(
            matches!(
                error,
                FactsError::Invalid {
                    field: "server_url",
                    ..
                }
            ),
            "{error}"
        );
        assert!(!error.to_string().contains("secret"), "{error}");
    }
    Ok(())
}

fn facts_of_repository(repository: &str) -> Result<TrustedFacts, FactsError> {
    let mut job = job().expect("fixture");
    let github = github(&mut job).expect("github context");
    github.retain(|entry| entry["k"] != "repository");
    github.push(entry("repository", &string(repository)));
    parse(&job)
}

#[test]
fn a_repository_named_with_a_leading_dot_has_facts_as_on_github() {
    for repository in ["tailrocks/.github", "tailrocks/.dotfiles", ".org/.github"] {
        let facts = facts_of_repository(repository).expect(repository);
        assert_eq!(facts.repository.as_str(), repository);
    }
}

#[test]
fn a_repository_part_that_names_a_directory_is_refused() {
    for repository in [
        "tailrocks/.",
        "tailrocks/..",
        "./velnor-new",
        "../velnor-new",
        "tailrocks/",
        "/velnor-new",
        "tailrocks/a b",
    ] {
        assert!(
            matches!(
                facts_of_repository(repository),
                Err(FactsError::Invalid {
                    field: "repository",
                    ..
                })
            ),
            "{repository}"
        );
    }
}
