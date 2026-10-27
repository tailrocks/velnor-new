// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use serde_json::{Value, json};
use velnor_actions_contract::WorkflowEvent;

#[path = "group_ledger_source_scope/fixture.rs"]
mod fixture;
use fixture::Fixture;

const REPOSITORY: &str = "tailrocks/velnor-new";
const GROUP: &str = "scope-group";
const RUN_ID: u64 = 501;
const RUN_ATTEMPT: u64 = 1;
const BASE_SHA: &str = "1111111111111111111111111111111111111111";
const PR_HEAD: &str = "2222222222222222222222222222222222222222";
const OTHER_HEAD: &str = "3333333333333333333333333333333333333333";
const MERGE_SHA: &str = "4444444444444444444444444444444444444444";
const WORKFLOW_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Clone, Copy)]
enum Kind {
    PullRequest,
    Fork,
    Qualification,
}

impl Kind {
    fn request_event(self) -> &'static str {
        match self {
            Self::PullRequest | Self::Fork => "pull_request",
            Self::Qualification => "workflow_dispatch",
        }
    }

    fn logical_event(self) -> WorkflowEvent {
        match self {
            Self::PullRequest => WorkflowEvent::PullRequest,
            Self::Fork => WorkflowEvent::Fork,
            Self::Qualification => WorkflowEvent::Qualification,
        }
    }

    fn logical_head(self) -> &'static str {
        match self {
            Self::PullRequest | Self::Fork => PR_HEAD,
            Self::Qualification => MERGE_SHA,
        }
    }

    fn payload(self) -> Value {
        let repository = json!({"id":819_273,"full_name":REPOSITORY,"default_branch":"main"});
        match self {
            Self::PullRequest | Self::Fork => json!({
                "repository":repository,
                "pull_request":{
                    "number":17,
                    "base":{"sha":BASE_SHA,"ref":"main","repo":{"id":819_273}},
                    "head":{
                        "sha":PR_HEAD,"ref":"fixture-head",
                        "repo":{
                            "id":if matches!(self,Self::Fork){90_210}else{819_273},
                            "full_name":if matches!(self,Self::Fork){"fixture/fork"}else{REPOSITORY},
                            "fork":matches!(self,Self::Fork)
                        }
                    }
                }
            }),
            Self::Qualification => json!({
                "repository":repository,
                "ref":"refs/heads/main",
                "inputs":{"campaign":"source-scope","phase":"cold"}
            }),
        }
    }
}

#[derive(Clone, Copy)]
enum Mutation {
    MissingPayload,
    MalformedPayload,
    ChangedHead,
    ChangedEvent,
}

#[test]
fn public_emit_binds_pull_request_ledger_to_logical_head() -> Result<(), Box<dyn Error>> {
    assert_success(Kind::PullRequest)
}

#[test]
fn public_emit_normalizes_fork_pull_request_event() -> Result<(), Box<dyn Error>> {
    assert_success(Kind::Fork)
}

#[test]
fn public_emit_normalizes_dispatch_event_to_qualification() -> Result<(), Box<dyn Error>> {
    assert_success(Kind::Qualification)
}

#[test]
fn public_emit_rejects_missing_event_after_request_capture() -> Result<(), Box<dyn Error>> {
    assert_rejected(Kind::PullRequest, Mutation::MissingPayload)
}

#[test]
fn public_emit_rejects_malformed_event_after_request_capture() -> Result<(), Box<dyn Error>> {
    assert_rejected(Kind::PullRequest, Mutation::MalformedPayload)
}

#[test]
fn public_emit_rejects_changed_logical_head_after_request_capture() -> Result<(), Box<dyn Error>> {
    assert_rejected(Kind::PullRequest, Mutation::ChangedHead)
}

#[test]
fn public_emit_rejects_changed_event_after_request_capture() -> Result<(), Box<dyn Error>> {
    assert_rejected(Kind::PullRequest, Mutation::ChangedEvent)
}

fn assert_success(kind: Kind) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(kind)?;
    let request = fixture.write_request(kind)?;
    let expected_event = match kind.logical_event() {
        WorkflowEvent::PullRequest => "pull_request",
        WorkflowEvent::Fork => "fork",
        WorkflowEvent::Qualification => "qualification",
        _ => return Err("unexpected event case".into()),
    };
    assert_eq!(request["event"], expected_event);
    assert_eq!(request["head"], kind.logical_head());
    if matches!(kind, Kind::PullRequest | Kind::Fork) {
        assert_ne!(MERGE_SHA, kind.logical_head());
    }
    let output = fixture.emit(kind.request_event())?;
    if !output.status.success() {
        return Err(format!(
            "public emit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let ledger = fixture.read_ledger()?;
    assert_eq!(ledger.job.event, expected_event);
    assert_eq!(ledger.job.source_sha, kind.logical_head());
    assert_eq!(
        ledger.job.event,
        request["event"].as_str().ok_or("request event missing")?
    );
    assert_eq!(
        ledger.job.source_sha,
        request["head"].as_str().ok_or("request head missing")?
    );
    Ok(())
}

fn assert_rejected(kind: Kind, mutation: Mutation) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(kind)?;
    let _request = fixture.write_request(kind)?;
    match mutation {
        Mutation::MissingPayload => fs::remove_file(&fixture.event_path)?,
        Mutation::MalformedPayload => fs::write(&fixture.event_path, b"{")?,
        Mutation::ChangedHead => fs::write(
            &fixture.event_path,
            kind.payload().to_string().replace(PR_HEAD, OTHER_HEAD),
        )?,
        Mutation::ChangedEvent => {
            let push = json!({"before":BASE_SHA,"after":PR_HEAD});
            fs::write(&fixture.event_path, push.to_string())?;
        }
    }
    let event_name = if matches!(mutation, Mutation::ChangedEvent) {
        "push"
    } else {
        kind.request_event()
    };
    let output = fixture.emit(event_name)?;
    assert!(!output.status.success(), "mutated event was accepted");
    assert!(
        !fixture.ledger_path().exists(),
        "rejected event left a group ledger"
    );
    Ok(())
}
