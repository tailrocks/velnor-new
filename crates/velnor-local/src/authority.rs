// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::protocol::Error;
use crate::side::Side;
use crate::standin::{Job, Run};

pub const WARNING: &str = "WARNING: the approved-workflows check is off in local mode, and jobs run the checkout's workflows as they are";
pub const WARNING_POLICY: &str = "WARNING: the approved-workflows check is on (--policy): a job the policy file does not approve is refused at job start with reason policy; fork pull requests, merge queue refs and workflows that call other workflows are never approved";
pub const RUNS_PATH: &str = "/_apis/v1/Message/workflow/runs";
pub const AGENTS_PATH: &str = "/_apis/v1/Agent/0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub workflow: String,
    pub run: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub side: Side,
    pub event: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub sha: String,
    pub workflow: Option<String>,
    pub existing: Vec<u64>,
    #[serde(default)]
    pub claims: Vec<Claim>,
}

impl Entry {
    fn holds(&self, facts: &Facts) -> bool {
        self.claims
            .iter()
            .any(|claim| claim.workflow == facts.workflow && claim.run == facts.id)
    }

    fn describes(&self, event: &str, git_ref: &str, sha: &str, workflow: &str) -> bool {
        self.event == event
            && self.git_ref == git_ref
            && self.sha == sha
            && self
                .workflow
                .as_deref()
                .is_none_or(|bound| bound == workflow)
    }

    fn open_for(&self, workflow: &str) -> bool {
        self.claims.iter().all(|claim| claim.workflow != workflow)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Posted {
    pub id: String,
    pub side: Side,
    pub event: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub sha: String,
    pub workflow: Option<String>,
}

impl Posted {
    pub fn entry(self, existing: Vec<u64>) -> Result<Entry, Error> {
        let named = [&self.id, &self.event, &self.git_ref, &self.sha];
        if named.iter().any(|value| value.is_empty()) {
            return Err(Error::invalid("id, event, ref and sha must not be empty"));
        }
        let plain = |byte: u8| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte);
        if self.id.len() > 64 || !self.id.bytes().all(plain) {
            return Err(Error::invalid("id is 1 to 64 letters, digits, - _ or ."));
        }
        Ok(Entry {
            id: self.id,
            side: self.side,
            event: self.event,
            git_ref: self.git_ref,
            sha: self.sha,
            workflow: self.workflow,
            existing,
            claims: Vec::new(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JitRequest {
    pub side: Side,
    pub name: String,
}

#[must_use]
pub fn agent_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call<'a> {
    Ledger,
    Jit,
    Run { side: Side, id: u64, attempt: u32 },
    Agent { side: Side, name: &'a str },
    Unknown,
}

#[must_use]
pub fn call<'a>(method: &str, path: &'a str) -> Call<'a> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method, parts[..].as_ref()) {
        ("POST", ["ledger"]) => Call::Ledger,
        ("POST", ["jit"]) => Call::Jit,
        ("GET", ["runs", side, id, attempt]) => {
            match (Side::parse(side), id.parse(), attempt.parse()) {
                (Some(side), Ok(id), Ok(attempt)) => Call::Run { side, id, attempt },
                _ => Call::Unknown,
            }
        }
        ("DELETE", ["agents", side, name]) if agent_name(name) => {
            Side::parse(side).map_or(Call::Unknown, |side| Call::Agent { side, name })
        }
        _ => Call::Unknown,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Facts {
    pub id: u64,
    pub attempt: u32,
    pub event: String,
    pub git_ref: String,
    pub sha: String,
    pub workflow: String,
    pub jobs: Vec<String>,
}

#[must_use]
pub fn facts(runs: &[Run], jobs: &[Job], id: u64, attempt: u32) -> Option<Facts> {
    let run = runs.iter().find(|run| run.id == id)?;
    let ours: Vec<&Job> = jobs
        .iter()
        .filter(|job| job.runid == id && job.attempt == attempt)
        .collect();
    if ours.is_empty() {
        return None;
    }
    Some(Facts {
        id,
        attempt,
        event: run.event_name.clone().unwrap_or_default(),
        git_ref: run.git_ref.clone().unwrap_or_default(),
        sha: run.sha.clone().unwrap_or_default(),
        workflow: run.file_name.clone(),
        jobs: ours
            .iter()
            .map(|job| {
                job.workflow_identifier
                    .clone()
                    .unwrap_or_else(|| job.name.clone())
            })
            .collect(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotDispatched,
    WrongSide,
}

impl Refusal {
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::NotDispatched => "not_dispatched",
            Self::WrongSide => "wrong_side",
        }
    }
}

fn describes(entry: &Entry, facts: &Facts) -> bool {
    entry.describes(&facts.event, &facts.git_ref, &facts.sha, &facts.workflow)
}

fn is_look_alike(entry: &Entry, run: &Run) -> bool {
    entry.describes(
        run.event_name.as_deref().unwrap_or_default(),
        run.git_ref.as_deref().unwrap_or_default(),
        run.sha.as_deref().unwrap_or_default(),
        &run.file_name,
    )
}

pub fn admit<'a>(
    entries: &'a [Entry],
    side: Side,
    facts: &Facts,
    runs: &[Run],
) -> Result<&'a Entry, Refusal> {
    let own = |entry: &&Entry| {
        entry.side == side && describes(entry, facts) && !entry.existing.contains(&facts.id)
    };
    if let Some(entry) = entries
        .iter()
        .filter(own)
        .find(|entry| entry.holds(facts))
        .or_else(|| next_free(entries, side, facts, runs))
    {
        return Ok(entry);
    }
    if entries
        .iter()
        .any(|entry| entry.side != side && describes(entry, facts))
    {
        return Err(Refusal::WrongSide);
    }
    Err(Refusal::NotDispatched)
}

fn next_free<'a>(
    entries: &'a [Entry],
    side: Side,
    facts: &Facts,
    runs: &[Run],
) -> Option<&'a Entry> {
    let mut taken: Vec<u64> = entries
        .iter()
        .filter(|entry| entry.side == side)
        .flat_map(|entry| &entry.claims)
        .filter(|claim| claim.workflow == facts.workflow)
        .map(|claim| claim.run)
        .collect();
    for entry in entries.iter().filter(|entry| entry.side == side) {
        if !entry.open_for(&facts.workflow) || !describes(entry, facts) {
            continue;
        }
        let Some(first) = runs
            .iter()
            .filter(|run| run.file_name == facts.workflow && !taken.contains(&run.id))
            .filter(|run| !entry.existing.contains(&run.id) && is_look_alike(entry, run))
            .map(|run| run.id)
            .min()
        else {
            continue;
        };
        if first == facts.id {
            return Some(entry);
        }
        taken.push(first);
    }
    None
}

pub fn claim(entries: &mut [Entry], id: &str, facts: &Facts) -> bool {
    let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
        return false;
    };
    if entry.holds(facts) {
        return false;
    }
    entry.claims.push(Claim {
        workflow: facts.workflow.clone(),
        run: facts.id,
    });
    true
}

#[must_use]
pub fn warning(policy: bool) -> &'static str {
    if policy { WARNING_POLICY } else { WARNING }
}

#[must_use]
pub fn answer(
    side: Side,
    facts: &Facts,
    admitted: Result<&Entry, Refusal>,
    workflow_sha256: Option<&str>,
) -> Value {
    json!({
        "side": side.name(),
        "id": facts.id,
        "attempt": facts.attempt,
        "event": facts.event,
        "ref": facts.git_ref,
        "sha": facts.sha,
        "workflow": facts.workflow,
        "jobs": facts.jobs,
        "ledger": admitted.map(|entry| entry.id.as_str()).ok(),
        "refusal": admitted.err().map(Refusal::code),
        "workflow_sha256": workflow_sha256,
    })
}

/// The runner inventory was not a valid listing, so it proves nothing either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedInventory;

/// `Ok(None)` means the inventory is valid and does not list `name`.
pub fn agent_id(listing: &[u8], name: &str) -> Result<Option<u64>, MalformedInventory> {
    let listing: Value = serde_json::from_slice(listing).map_err(|_| MalformedInventory)?;
    let agents = listing["value"].as_array().ok_or(MalformedInventory)?;
    Ok(agents
        .iter()
        .find(|agent| agent["name"] == name)
        .and_then(|agent| agent["id"].as_u64()))
}

#[must_use]
pub fn run_ids(runs: &[Run]) -> Vec<u64> {
    runs.iter().map(|run| run.id).collect()
}
