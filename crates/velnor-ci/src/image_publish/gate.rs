// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::error::CiError;
use crate::image_candidate::json::is_hex;

pub const REPOSITORY: &str = "tailrocks/velnor-new";
pub const REPOSITORY_ID: u64 = 1_390_620_900;
pub const MAIN_REF: &str = "refs/heads/main";
pub const DEFAULT_WORKFLOW: &str = "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml";
pub const DEFAULT_EVENT: &str = "workflow_dispatch";
pub const REQUIRED: &[&str] = &[
    "GITHUB_REPOSITORY",
    "GITHUB_REPOSITORY_ID",
    "GITHUB_REF",
    "GITHUB_EVENT_NAME",
    "GITHUB_WORKFLOW_REF",
    "GITHUB_SHA",
    "GITHUB_WORKFLOW_SHA",
    "GITHUB_RUN_ID",
    "GITHUB_RUN_ATTEMPT",
    "VELNOR_CANDIDATE_ARTIFACT_ID",
];
const WORKFLOW_DIRECTORY: &str = "tailrocks/velnor-new/.github/workflows/";
const EVENTS: &[&str] = &["workflow_dispatch", "push"];

fn fail<T>(message: impl Into<String>) -> Result<T, CiError> {
    Err(CiError::new(message))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    workflow: String,
    event: String,
}

impl Policy {
    pub fn new(workflow: &str, event: &str) -> Result<Self, CiError> {
        let file = workflow
            .strip_prefix(WORKFLOW_DIRECTORY)
            .unwrap_or_default();
        let named = !file.is_empty()
            && Path::new(file)
                .extension()
                .is_some_and(|extension| extension == "yml" || extension == "yaml")
            && file
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'));
        if !named {
            return fail(format!(
                "Publishing workflow must be a file under {WORKFLOW_DIRECTORY}."
            ));
        }
        if !EVENTS.contains(&event) {
            return fail("Publishing event must be workflow_dispatch or push.");
        }
        Ok(Self {
            workflow: workflow.to_owned(),
            event: event.to_owned(),
        })
    }

    #[must_use]
    pub fn workflow(&self) -> &str {
        &self.workflow
    }

    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    #[must_use]
    pub fn workflow_ref(&self) -> String {
        format!("{}@{MAIN_REF}", self.workflow)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub source: String,
    pub workflow_ref: String,
    pub run_id: u64,
    pub run_attempt: u64,
    pub artifact_id: u64,
}

fn number(text: &str, label: &str) -> Result<u64, CiError> {
    let positive = text.bytes().all(|byte| byte.is_ascii_digit()) && !text.starts_with('0');
    match text.parse::<u64>() {
        Ok(value) if positive => Ok(value),
        _ => fail(format!("{label} is incorrect.")),
    }
}

fn commit(text: &str, label: &str) -> Result<String, CiError> {
    if is_hex(text, 40) {
        Ok(text.to_owned())
    } else {
        fail(format!("{label} is incorrect."))
    }
}

pub fn identity<S: BuildHasher>(
    env: &HashMap<String, String, S>,
    policy: &Policy,
) -> Result<Identity, CiError> {
    let get = |name: &str| env.get(name).map_or("", String::as_str);
    if REQUIRED.iter().any(|name| get(name).is_empty()) {
        return fail("Required GitHub workflow identity is missing.");
    }
    if get("GITHUB_REPOSITORY") != REPOSITORY {
        return fail("Publication repository is incorrect.");
    }
    if get("GITHUB_REF") != MAIN_REF || get("GITHUB_EVENT_NAME") != policy.event() {
        return fail(format!(
            "Publication is only for {} on {MAIN_REF}.",
            policy.event()
        ));
    }
    if get("GITHUB_WORKFLOW_REF") != policy.workflow_ref() {
        return fail("Publication workflow authority is incorrect.");
    }
    let source = commit(get("GITHUB_SHA"), "Source SHA")?;
    if commit(get("GITHUB_WORKFLOW_SHA"), "Workflow authority SHA")? != source {
        return fail("Workflow authority SHA is different from source SHA.");
    }
    if number(get("GITHUB_REPOSITORY_ID"), "GitHub repository ID")? != REPOSITORY_ID {
        return fail("GitHub repository ID is incorrect.");
    }
    Ok(Identity {
        source,
        workflow_ref: get("GITHUB_WORKFLOW_REF").to_owned(),
        run_id: number(get("GITHUB_RUN_ID"), "Publication run ID")?,
        run_attempt: number(get("GITHUB_RUN_ATTEMPT"), "Publication run attempt")?,
        artifact_id: number(get("VELNOR_CANDIDATE_ARTIFACT_ID"), "Candidate artifact ID")?,
    })
}

pub fn record_matches(
    record: &Map<String, Value>,
    identity: &Identity,
    policy: &Policy,
) -> Result<(), CiError> {
    if record["source"]["commit"] != identity.source || record["source"]["ref"] != MAIN_REF {
        return fail("Candidate source is not the current main source.");
    }
    let expected = json!({
        "repository": REPOSITORY,
        "repository_id": REPOSITORY_ID,
        "event": policy.event(),
        "workflow_ref": identity.workflow_ref,
        "authority_commit": identity.source,
        "run_id": identity.run_id,
        "run_attempt": identity.run_attempt,
    });
    if record["workflow"] != expected {
        return fail("Candidate run identity is different from the publication run identity.");
    }
    Ok(())
}
