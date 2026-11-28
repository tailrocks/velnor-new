// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{
    ApiPage, ApiRule, GitHub, Repo, compare_path, encode, parse, rules_path, run_jobs_path,
};
use crate::merge_group::workflow_file;
use crate::private_token::parse_queue_ref;
use crate::verdict::{
    Evidence, JobEvidence, Refusal, Reusable, Reuse, RunEvidence, VerdictMatch, decide,
};

const JOB_PAGES: u32 = 5;
const RULE_PAGES: u32 = 5;
const RUNS_TRIED: usize = 10;
const NON_FAST_FORWARD: &str = "non_fast_forward";

#[derive(Debug)]
pub struct Context {
    pub repository: String,
    pub workflow_ref: String,
    pub head_ref: String,
    pub base_sha: String,
    pub required: String,
    pub server_url: String,
}

pub trait Artifacts {
    fn verdict_text(&self, repo: &Repo, run: u64) -> Result<String, CiError>;
}

#[derive(Deserialize)]
struct Pull {
    head: PullHead,
}

#[derive(Deserialize)]
struct PullHead {
    sha: String,
    repo: Option<PullRepo>,
}

#[derive(Deserialize)]
struct PullRepo {
    full_name: String,
}

#[derive(Deserialize)]
struct Comparison {
    behind_by: u64,
}

#[derive(Deserialize)]
struct Commit {
    tree: Tree,
}

#[derive(Deserialize)]
struct Tree {
    sha: String,
}

#[derive(Deserialize)]
struct PullRequestRun {
    id: u64,
    event: String,
    status: String,
    head_sha: String,
    #[serde(default)]
    pull_requests: Vec<Linked>,
}

#[derive(Deserialize)]
struct Linked {
    number: u64,
}

#[derive(Deserialize)]
struct RunJobs {
    jobs: Vec<RunJob>,
}

#[derive(Deserialize)]
struct RunJob {
    name: String,
    run_attempt: u32,
    conclusion: Option<String>,
    #[serde(default)]
    steps: Vec<RunStep>,
}

#[derive(Deserialize)]
struct RunStep {
    name: String,
    conclusion: Option<String>,
}

#[must_use]
pub fn pull_path(repo: &Repo, number: u64) -> String {
    format!("/repos/{repo}/pulls/{number}")
}

#[must_use]
pub fn commit_path(repo: &Repo, sha: &str) -> String {
    format!("/repos/{repo}/git/commits/{sha}")
}

#[must_use]
pub fn pull_request_runs_path(repo: &Repo, workflow_file: &str, sha: &str) -> String {
    format!(
        "/repos/{repo}/actions/workflows/{}/runs?head_sha={sha}&event=pull_request&per_page=100",
        encode(workflow_file)
    )
}

fn read<T: serde::de::DeserializeOwned>(
    github: &dyn GitHub,
    path: &str,
    what: &'static str,
) -> Result<T, Refusal> {
    let unreadable = |error: CiError| Refusal::Unreadable {
        what,
        reason: error.to_string(),
    };
    parse(&github.get(path).map_err(unreadable)?, what).map_err(unreadable)
}

fn forbids_force_push(github: &dyn GitHub, repo: &Repo, branch: &str) -> Result<bool, Refusal> {
    for page in 1..=RULE_PAGES {
        let rules: Vec<ApiRule> = read(github, &rules_path(repo, branch, page), "branch rules")?;
        if rules.iter().any(|rule| rule.kind == NON_FAST_FORWARD) {
            return Ok(true);
        }
        if rules.len() < 100 {
            break;
        }
    }
    Ok(false)
}

fn jobs_of(github: &dyn GitHub, repo: &Repo, run: u64) -> Result<Vec<JobEvidence>, Refusal> {
    let mut found = Vec::new();
    for page in 1..=JOB_PAGES {
        let jobs: RunJobs = read(github, &run_jobs_path(repo, run, page), "jobs")?;
        let last = jobs.jobs.len() < 100;
        found.extend(jobs.jobs.into_iter().map(|job| {
            JobEvidence {
                name: job.name,
                attempt: job.run_attempt,
                conclusion: job.conclusion,
                steps: job
                    .steps
                    .into_iter()
                    .map(|step| (step.name, step.conclusion))
                    .collect(),
            }
        }));
        if last {
            break;
        }
    }
    Ok(found)
}

fn passed_run(
    github: &dyn GitHub,
    repo: &Repo,
    file: &str,
    head: &str,
    number: u64,
    required: &str,
) -> Result<Option<(u64, Vec<JobEvidence>)>, Refusal> {
    let page: ApiPage<PullRequestRun> = read(
        github,
        &pull_request_runs_path(repo, file, head),
        "workflow runs",
    )?;
    let mut runs: Vec<&PullRequestRun> = page
        .items
        .iter()
        .filter(|run| {
            run.event == "pull_request"
                && run.status == "completed"
                && run.head_sha == head
                && run
                    .pull_requests
                    .iter()
                    .any(|linked| linked.number == number)
        })
        .collect();
    runs.sort_by_key(|run| std::cmp::Reverse(run.id));
    for run in runs.into_iter().take(RUNS_TRIED) {
        let jobs = jobs_of(github, repo, run.id)?;
        let required_passed = jobs
            .iter()
            .any(|job| job.name == required && job.conclusion.as_deref() == Some("success"));
        if required_passed {
            return Ok(Some((run.id, jobs)));
        }
    }
    Ok(None)
}

pub fn gather(
    github: &dyn GitHub,
    artifacts: &dyn Artifacts,
    context: &Context,
) -> Result<Evidence, Refusal> {
    let entry = parse_queue_ref(&context.head_ref)
        .map_err(|error| Refusal::NotAQueueEntry(error.to_string()))?;
    let repo = Repo::parse(&context.repository).map_err(|error| Refusal::Unreadable {
        what: "the repository",
        reason: error.to_string(),
    })?;
    let file = workflow_file(&context.repository, &context.workflow_ref).ok_or_else(|| {
        Refusal::Unreadable {
            what: "the workflow",
            reason: "GITHUB_WORKFLOW_REF does not name a workflow of this repository".to_owned(),
        }
    })?;
    let pull: Pull = read(github, &pull_path(&repo, entry.number), "the pull request")?;
    if pull.head.sha != entry.sha {
        return Err(Refusal::HeadMoved {
            queued: entry.sha,
            live: pull.head.sha,
        });
    }
    let head = pull.head.sha;
    let comparison: Comparison = read(
        github,
        &compare_path(&repo, &context.base_sha, &head),
        "the comparison",
    )?;
    let commit: Commit = read(github, &commit_path(&repo, &head), "the head commit")?;
    let forbids = forbids_force_push(github, &repo, &entry.base)?;
    let run = passed_run(github, &repo, file, &head, entry.number, &context.required)?.map(
        |(id, jobs)| RunEvidence {
            url: format!("{}/{repo}/actions/runs/{id}", context.server_url),
            jobs,
            verdict: artifacts
                .verdict_text(&repo, id)
                .map_err(|error| error.to_string())
                .and_then(|text| VerdictMatch::parse(&text)),
        },
    );
    Ok(Evidence {
        same_repository: pull
            .head
            .repo
            .is_some_and(|head_repo| head_repo.full_name == context.repository),
        head_tree: commit.tree.sha,
        behind_by: comparison.behind_by,
        target: entry.base,
        forbids_force_push: forbids,
        run,
    })
}

pub fn evaluate(
    github: &dyn GitHub,
    artifacts: &dyn Artifacts,
    context: &Context,
    own: &VerdictMatch,
    reusable: &[Reusable],
) -> Result<Reuse, Refusal> {
    decide(&gather(github, artifacts, context)?, own, reusable)
}
