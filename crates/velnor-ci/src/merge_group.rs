// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::github::{
    ApiJobs, ApiMergeGroupRun, ApiPage, GitHub, Repo, merge_group_runs_path, parse, run_jobs_path,
};

const JOB_PAGES: u32 = 5;

#[derive(Debug)]
pub struct Context {
    pub event: String,
    pub repository: String,
    pub sha: String,
    pub workflow_ref: String,
    pub required: Option<String>,
    pub lock_changed: Option<String>,
}

#[derive(Debug)]
pub struct SkipPush {
    pub skip: bool,
    pub note: String,
}

fn run_everything(why: impl Into<String>) -> SkipPush {
    SkipPush {
        skip: false,
        note: format!("running everything: {}", why.into()),
    }
}

fn workflow_file(context: &Context) -> Option<&str> {
    let path = context
        .workflow_ref
        .strip_prefix(&context.repository)?
        .strip_prefix('/')?
        .split_once("@refs/")?
        .0;
    let file = path.strip_prefix(".github/workflows/")?;
    (!file.is_empty() && !file.contains('/')).then_some(file)
}

const LOCK_FILE: &str = "Cargo.lock";
const DIFF_FAILED: &str = "<unknown: git diff failed>";

#[must_use]
pub fn changed_lock(diff: Option<&[u8]>) -> Option<String> {
    let Some(diff) = diff else {
        return Some(DIFF_FAILED.to_owned());
    };
    diff.split(|byte| *byte == 0)
        .map(String::from_utf8_lossy)
        .find(|path| path.rsplit('/').next() == Some(LOCK_FILE))
        .map(std::borrow::Cow::into_owned)
}

fn commit(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Never fails: whatever it cannot establish means "run everything".
#[must_use]
pub fn skip_push(github: &impl GitHub, context: &Context) -> SkipPush {
    if context.event != "push" {
        return run_everything(format!("the event is {}, not push", context.event));
    }
    if !commit(&context.sha) {
        return run_everything("GITHUB_SHA is not a commit id");
    }
    if let Some(path) = &context.lock_changed {
        return run_everything(format!(
            "{path} changed, so this push builds and saves the base for the new lock"
        ));
    }
    let Ok(repo) = Repo::parse(&context.repository) else {
        return run_everything("GITHUB_REPOSITORY is not owner/name");
    };
    let Some(file) = workflow_file(context) else {
        return run_everything("GITHUB_WORKFLOW_REF does not name a workflow of this repository");
    };
    let body = match github.get(&merge_group_runs_path(&repo, file, &context.sha)) {
        Ok(body) => body,
        Err(error) => return run_everything(error.to_string()),
    };
    let page: ApiPage<ApiMergeGroupRun> = match parse(&body, "workflow runs") {
        Ok(page) => page,
        Err(error) => return run_everything(error.to_string()),
    };
    let latest = page
        .items
        .iter()
        .filter(|run| run.event == "merge_group" && run.head_sha == context.sha)
        .max_by_key(|run| run.id);
    let passed = latest.is_some_and(|run| match context.required.as_deref() {
        Some(name) => required_job_passed(github, &repo, run.id, name),
        None => run.status == "completed" && run.conclusion.as_deref() == Some("success"),
    });
    if passed {
        SkipPush {
            skip: true,
            note: format!(
                "a merge_group run of {file} already passed on {}; skipping the checks",
                context.sha
            ),
        }
    } else {
        run_everything(format!(
            "no passed merge_group run of {file} on {}",
            context.sha
        ))
    }
}

fn required_job_passed(github: &impl GitHub, repo: &Repo, run: u64, name: &str) -> bool {
    for page in 1..=JOB_PAGES {
        let Ok(body) = github.get(&run_jobs_path(repo, run, page)) else {
            return false;
        };
        let Ok(jobs) = parse::<ApiJobs>(&body, "jobs") else {
            return false;
        };
        if jobs
            .jobs
            .iter()
            .any(|job| job.name == name && job.conclusion.as_deref() == Some("success"))
        {
            return true;
        }
        if jobs.jobs.len() < 100 {
            return false;
        }
    }
    false
}
