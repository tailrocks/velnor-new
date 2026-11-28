// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use velnor_model::REUSE_RUN_MAX_BYTES;

use crate::error::CiError;

pub const FORMAT: u32 = 2;
pub const ARTIFACT_NAME: &str = "velnor-verdict";
pub const FILE_NAME: &str = "verdict.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerdictMatch {
    pub format: u32,
    pub attempt: u32,
    pub tree: String,
    pub plan: BTreeMap<String, String>,
}

impl VerdictMatch {
    #[must_use]
    pub fn new(attempt: u32, tree: String, plan: BTreeMap<String, String>) -> Self {
        Self {
            format: FORMAT,
            attempt,
            tree,
            plan,
        }
    }

    pub fn parse_plan(text: &str) -> Result<BTreeMap<String, String>, CiError> {
        serde_json::from_str(text).map_err(|e| {
            CiError::new(format!(
                "the plan outputs are not a JSON object of strings: {e}"
            ))
        })
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let parsed: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if parsed.format == FORMAT {
            Ok(parsed)
        } else {
            Err(format!("format {} is not {FORMAT}", parsed.format))
        }
    }

    pub fn text(&self) -> Result<String, CiError> {
        serde_json::to_string(self)
            .map_err(|e| CiError::new(format!("cannot encode the verdict: {e}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reusable {
    pub id: String,
    pub job: String,
    #[serde(default)]
    pub step: Option<String>,
}

impl Reusable {
    pub fn parse_list(text: &str) -> Result<Vec<Self>, CiError> {
        serde_json::from_str(text)
            .map_err(|e| CiError::new(format!("the reusable list is not readable: {e}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobEvidence {
    pub name: String,
    pub attempt: u32,
    pub conclusion: Option<String>,
    pub steps: Vec<(String, Option<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEvidence {
    pub url: String,
    pub jobs: Vec<JobEvidence>,
    pub verdict: Result<VerdictMatch, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub same_repository: bool,
    pub head_tree: String,
    pub behind_by: u64,
    pub target: String,
    pub forbids_force_push: bool,
    pub run: Option<RunEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotAQueueEntry(String),
    Unreadable { what: &'static str, reason: String },
    ForkPullRequest,
    BehindBase(u64),
    OtherTreeThanHead { head: String, own: String },
    ForcePushAllowed(String),
    NoPassedRun,
    NoVerdict(String),
    OtherAttempt { recorded: u32, latest: u32 },
    OtherTree,
    OtherPlan,
    NoAttempt,
    HeadMoved { queued: String, live: String },
    RunUrlTooLong,
    AmbiguousJob(String),
    AmbiguousStep { job: String, step: String },
    NothingToReuse,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAQueueEntry(reason) => write!(f, "this run is not a queue entry: {reason}"),
            Self::Unreadable { what, reason } => write!(f, "cannot read {what}: {reason}"),
            Self::ForkPullRequest => f.write_str("the pull request is not from this repository"),
            Self::BehindBase(behind) => write!(
                f,
                "the pull request head is {behind} commits behind the queue base"
            ),
            Self::OtherTreeThanHead { head, own } => write!(
                f,
                "the pull request head has tree {head}, this checkout has {own}"
            ),
            Self::ForcePushAllowed(branch) => {
                write!(f, "{branch} has no active non_fast_forward rule")
            }
            Self::NoPassedRun => f.write_str(
                "no pull_request run of this workflow on that head passed its Required check",
            ),
            Self::NoVerdict(reason) => write!(f, "the run has no usable velnor-verdict: {reason}"),
            Self::OtherAttempt { recorded, latest } => write!(
                f,
                "the verdict is of attempt {recorded}, the run's latest attempt is {latest}"
            ),
            Self::OtherTree => f.write_str("the verdict is for another tree"),
            Self::OtherPlan => f.write_str("the verdict is for another plan"),
            Self::NoAttempt => f.write_str("the run's jobs report no run attempt"),
            Self::HeadMoved { queued, live } => write!(
                f,
                "the pull request head is {live}, not the queued commit {queued}"
            ),
            Self::RunUrlTooLong => f.write_str("the run URL is longer than the output allows"),
            Self::AmbiguousJob(job) => write!(
                f,
                "more than one job of the run is named {job}, so a job conclusion cannot be told apart"
            ),
            Self::AmbiguousStep { job, step } => write!(
                f,
                "job {job} has more than one step named {step}, so a step conclusion cannot be told apart"
            ),
            Self::NothingToReuse => f.write_str("no reusable step or job passed in that run"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reuse {
    pub run: String,
    pub reused: BTreeSet<String>,
}

fn passed(conclusion: Option<&String>) -> bool {
    conclusion.map(String::as_str) == Some("success")
}

fn concluded_success(run: &RunEvidence, item: &Reusable) -> bool {
    run.jobs.iter().any(|job| {
        job.name == item.job
            && passed(job.conclusion.as_ref())
            && item.step.as_ref().is_none_or(|step| {
                job.steps
                    .iter()
                    .any(|(name, conclusion)| name == step && passed(conclusion.as_ref()))
            })
    })
}

fn ambiguous_step(run: &RunEvidence, reusable: &[Reusable]) -> Option<Refusal> {
    reusable.iter().find_map(|item| {
        if run.jobs.iter().filter(|job| job.name == item.job).count() > 1 {
            return Some(Refusal::AmbiguousJob(item.job.clone()));
        }
        let step = item.step.as_ref()?;
        run.jobs
            .iter()
            .filter(|job| job.name == item.job)
            .any(|job| job.steps.iter().filter(|(name, _)| name == step).count() > 1)
            .then(|| Refusal::AmbiguousStep {
                job: item.job.clone(),
                step: step.clone(),
            })
    })
}

pub fn decide(
    evidence: &Evidence,
    own: &VerdictMatch,
    reusable: &[Reusable],
) -> Result<Reuse, Refusal> {
    if !evidence.same_repository {
        return Err(Refusal::ForkPullRequest);
    }
    if evidence.behind_by != 0 {
        return Err(Refusal::BehindBase(evidence.behind_by));
    }
    if evidence.head_tree != own.tree {
        return Err(Refusal::OtherTreeThanHead {
            head: evidence.head_tree.clone(),
            own: own.tree.clone(),
        });
    }
    if !evidence.forbids_force_push {
        return Err(Refusal::ForcePushAllowed(evidence.target.clone()));
    }
    let run = evidence.run.as_ref().ok_or(Refusal::NoPassedRun)?;
    let recorded = run
        .verdict
        .as_ref()
        .map_err(|reason| Refusal::NoVerdict(reason.clone()))?;
    let latest = run.jobs.iter().map(|job| job.attempt).max().unwrap_or(0);
    if latest == 0 {
        return Err(Refusal::NoAttempt);
    }
    if recorded.attempt != latest {
        return Err(Refusal::OtherAttempt {
            recorded: recorded.attempt,
            latest,
        });
    }
    if recorded.tree != own.tree {
        return Err(Refusal::OtherTree);
    }
    if recorded.plan != own.plan {
        return Err(Refusal::OtherPlan);
    }
    if run.url.len() > REUSE_RUN_MAX_BYTES {
        return Err(Refusal::RunUrlTooLong);
    }
    if let Some(refusal) = ambiguous_step(run, reusable) {
        return Err(refusal);
    }
    let reused: BTreeSet<String> = reusable
        .iter()
        .filter(|item| concluded_success(run, item))
        .map(|item| item.id.clone())
        .collect();
    if reused.is_empty() {
        return Err(Refusal::NothingToReuse);
    }
    Ok(Reuse {
        run: run.url.clone(),
        reused,
    })
}
