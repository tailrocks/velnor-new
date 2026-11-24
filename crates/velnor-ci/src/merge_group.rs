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
    let passed = latest.is_some_and(|run| {
        if run.status == "completed" {
            run.conclusion.as_deref() == Some("success")
        } else {
            context
                .required
                .as_deref()
                .is_some_and(|name| required_job_passed(github, &repo, run.id, name))
        }
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
