// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::github_hosted;

use crate::error::{WorkflowsError, refuse};
use crate::helper_call::command_words;
use crate::plan::{Action, Body, Env, Job, Step};

const MAX_ENVIRONMENT_CHARACTERS: usize = 255;

pub(crate) fn environment(id: &str, name: Option<&str>) -> Result<(), WorkflowsError> {
    let Some(name) = name else {
        return Ok(());
    };
    if name.trim().is_empty() {
        return refuse(format!("job {id} environment must not be empty"));
    }
    if name.chars().count() > MAX_ENVIRONMENT_CHARACTERS {
        return refuse(format!(
            "job {id} environment is longer than {MAX_ENVIRONMENT_CHARACTERS} characters, which GitHub does not allow"
        ));
    }
    Ok(())
}

/// A job container or service container runs only on a Linux runner. A job that is not
/// GitHub-hosted also runs on the fork runner for a pull request from a fork.
pub(crate) fn containers_on_linux(
    runner: &str,
    fork_runner: &str,
    jobs: &[Job],
) -> Result<(), WorkflowsError> {
    for job in jobs
        .iter()
        .filter(|job| job.container.is_some() || !job.services.is_empty())
    {
        let primary = job.runner.as_deref().unwrap_or(runner);
        let fork = (!github_hosted(primary)).then_some(fork_runner);
        for label in std::iter::once(primary).chain(fork) {
            let label_lower = label.to_ascii_lowercase();
            if label_lower.starts_with("macos") || label_lower.starts_with("windows") {
                return refuse(format!(
                    "job {} uses a container or a service on the runner {label}, but GitHub runs job and service containers on Linux runners only",
                    job.id
                ));
            }
        }
    }
    Ok(())
}

const SCRIPT_RUNNERS: [&str; 9] = [
    "bash", "dash", "make", "node", "perl", "python", "python3", "ruby", "sh",
];

fn runs_script(run: &str) -> bool {
    command_words(run)
        .first()
        .is_some_and(|word| word.contains('/') || SCRIPT_RUNNERS.contains(&word.as_str()))
}

/// A step that runs a repository script or a local action never reads `github.token` in a job
/// that can write. Other commands, such as `gh release create`, are the job's own work.
pub(crate) fn write_token(
    job: &str,
    steps: &[Step],
    permissions: &[(String, String)],
) -> Result<(), WorkflowsError> {
    if !permissions.iter().any(|(_, level)| level == "write") {
        return Ok(());
    }
    for step in steps {
        let runs_repository_code = match &step.body {
            Body::Run(run) => runs_script(run),
            Body::Local(_) => true,
            Body::Uses(_) | Body::Helper(_) => false,
        };
        let reads_token = step.env.iter().any(
            |(_, value)| matches!(value, Env::Expression(inner) if inner.contains("github.token")),
        );
        if runs_repository_code && reads_token {
            return refuse(format!(
                "job {job} step {} reads github.token in a job with a write permission and runs repository code; give the job read permissions only, or run the command through velnor-ci",
                step.name
            ));
        }
    }
    Ok(())
}

/// A job that reads a secret outside a dispatch-only workflow holds no write permission, so
/// a run that nobody started by hand never carries a secret and a write token together.
pub(crate) fn secret_without_write(
    job: &str,
    steps: &[Step],
    permissions: &[(String, String)],
) -> Result<(), WorkflowsError> {
    let reads_secret = steps
        .iter()
        .flat_map(|step| &step.env)
        .any(|(_, value)| matches!(value, Env::Expression(inner) if inner.starts_with("secrets.")));
    if let Some((scope, _)) = permissions
        .iter()
        .find(|(_, level)| reads_secret && level == "write")
    {
        return refuse(format!(
            "job {job} reads a secret and holds the write permission {scope}; a workflow that runs on more than workflow_dispatch gives a job that reads a secret read permissions only"
        ));
    }
    Ok(())
}

/// The cache client rejects a key over 512 characters, and an expression is as long as the
/// value a caller gives it.
pub(crate) fn bounded_cache_keys(
    job: &str,
    body: &Body,
    with: &[(String, Env)],
) -> Result<(), WorkflowsError> {
    if !matches!(
        body,
        Body::Uses(Action::Cache | Action::CacheRestore | Action::CacheSave)
    ) {
        return Ok(());
    }
    for (name, value) in with {
        let key = name.eq_ignore_ascii_case("key") || name.eq_ignore_ascii_case("restore-keys");
        if key && matches!(value, Env::Expression(_)) {
            return refuse(format!(
                "job {job} sets the cache key {name} to an expression, whose length is not bounded; write it as text, with github.run_id, github.run_attempt and github.job as the only expressions"
            ));
        }
    }
    Ok(())
}
