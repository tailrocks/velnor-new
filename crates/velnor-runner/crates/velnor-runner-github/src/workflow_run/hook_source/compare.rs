//! Compare hook source values with exact GitHub records.

use super::super::{
    RepositoryTarget, WorkflowRun, get_git_commit, get_pull_request, get_workflow_run_attempt,
    verify_pull_request_run_association, verify_workflow_source,
};
use super::{ComparedHookSourceFacts, HookSourceError, UntrustedHookSourceInput, compare_pull};
use crate::{CompleteSourceMetadata, Transport};

/// Compare hook source values with an exact workflow attempt and current GitHub records.
///
/// # Errors
///
/// Gives an error when any source value is missing, mutable, or different.
pub fn compare_hook_source<T>(
    transport: &mut T,
    input: &UntrustedHookSourceInput,
    expected: RepositoryTarget<'_>,
    token: &str,
) -> Result<ComparedHookSourceFacts, HookSourceError>
where
    T: Transport + ?Sized,
{
    input.validate()?;
    verify_scope(input, expected)?;
    let run = get_workflow_run_attempt(
        transport,
        expected.owner,
        expected.name,
        input.run_id,
        input.run_attempt,
        token,
    )?;
    verify_run(input, &run, expected)?;
    verify_root_ref(input, &run, expected)?;
    verify_commits(transport, input, expected, token)?;
    let pull = verify_pull(transport, input, &run, expected, token)?;
    let job_workflow_sha = verify_job_workflow(input, &run, expected)?;
    Ok(ComparedHookSourceFacts {
        repository_id: input.repository_id,
        repository: input.repository.clone(),
        run_id: input.run_id,
        run_attempt: input.run_attempt,
        event_name: input.event_name.clone(),
        event_ref: input.event_ref.clone(),
        tested_sha: input.tested_sha.clone(),
        workflow_ref: input.workflow_ref.clone(),
        workflow_sha: input.workflow_sha.clone(),
        job_workflow_ref: input.job_workflow_ref.clone(),
        workflow_job_id: input.workflow_job_id.clone(),
        job_workflow_sha,
        pull_request: pull,
    })
}

fn verify_scope(
    input: &UntrustedHookSourceInput,
    expected: RepositoryTarget<'_>,
) -> Result<(), HookSourceError> {
    let name = format!("{}/{}", expected.owner, expected.name);
    if input.repository_id != expected.id || !input.repository.eq_ignore_ascii_case(&name) {
        return Err(HookSourceError::Mismatch);
    }
    Ok(())
}

fn verify_run(
    input: &UntrustedHookSourceInput,
    run: &WorkflowRun,
    expected: RepositoryTarget<'_>,
) -> Result<(), HookSourceError> {
    let name = format!("{}/{}", expected.owner, expected.name);
    if run.event != input.event_name
        || run.repository.id != expected.id
        || !run.repository.full_name.eq_ignore_ascii_case(&name)
        || (input.event_name != "pull_request" && run.head_sha != input.tested_sha)
    {
        return Err(HookSourceError::Mismatch);
    }
    Ok(())
}

fn verify_root_ref(
    input: &UntrustedHookSourceInput,
    run: &WorkflowRun,
    expected: RepositoryTarget<'_>,
) -> Result<(), HookSourceError> {
    let (path, reference) = input
        .workflow_ref
        .split_once('@')
        .ok_or(HookSourceError::InvalidInput)?;
    let prefix = format!("{}/{}", expected.owner, expected.name);
    let workflow_path = path
        .strip_prefix(&format!("{prefix}/"))
        .ok_or(HookSourceError::Mismatch)?;
    let (api_path, api_reference) = run
        .path
        .split_once('@')
        .map_or((run.path.as_str(), None), |(path, reference)| {
            (path, Some(reference))
        });
    let run_path = api_path.strip_prefix("./").unwrap_or(api_path);
    let run_path = run_path
        .strip_prefix(&format!("{prefix}/"))
        .unwrap_or(run_path);
    let expected_workflow_ref = if input.event_name == "merge_group" {
        reference.starts_with("refs/heads/") || reference.starts_with("refs/merge-queue/")
    } else {
        reference == input.event_ref
    };
    if !expected_workflow_ref
        || api_reference.is_some_and(|value| value != reference)
        || workflow_path != run_path
        || !workflow_path.starts_with(".github/workflows/")
    {
        return Err(HookSourceError::Mismatch);
    }
    Ok(())
}

fn verify_commits<T>(
    transport: &mut T,
    input: &UntrustedHookSourceInput,
    expected: RepositoryTarget<'_>,
    token: &str,
) -> Result<(), HookSourceError>
where
    T: Transport + ?Sized,
{
    get_git_commit(
        transport,
        expected.owner,
        expected.name,
        &input.workflow_sha,
        token,
    )?;
    let tested = get_git_commit(
        transport,
        expected.owner,
        expected.name,
        &input.tested_sha,
        token,
    )?;
    if let Some(pull) = &input.pull_request
        && (tested.parents.len() != 2
            || tested.parents[0].sha != pull.base.sha
            || tested.parents[1].sha != pull.head.sha)
    {
        return Err(HookSourceError::Mismatch);
    }
    Ok(())
}

fn verify_pull<T>(
    transport: &mut T,
    input: &UntrustedHookSourceInput,
    run: &WorkflowRun,
    expected: RepositoryTarget<'_>,
    token: &str,
) -> Result<Option<super::ComparedHookPullRequestFacts>, HookSourceError>
where
    T: Transport + ?Sized,
{
    let Some(event) = &input.pull_request else {
        return Ok(None);
    };
    verify_pull_request_run_association(run, event.number, expected.id, event.head.repository.id)
        .map_err(|_| HookSourceError::Mismatch)?;
    let pull = get_pull_request(
        transport,
        expected.owner,
        expected.name,
        event.number,
        token,
    )?;
    compare_pull(input, run, &pull, expected.id).map(Some)
}

fn verify_job_workflow(
    input: &UntrustedHookSourceInput,
    run: &WorkflowRun,
    expected: RepositoryTarget<'_>,
) -> Result<Option<String>, HookSourceError> {
    let verified = verify_workflow_source(
        CompleteSourceMetadata {
            owner_name: expected.owner,
            repository_name: expected.name,
            job_workflow_ref: &input.job_workflow_ref,
            workflow_run_id: input.run_id,
            event_name: &input.event_name,
        },
        run,
        expected,
    )
    .map_err(|_| HookSourceError::Mismatch)?;
    if same_workflow_source(&input.workflow_ref, &input.job_workflow_ref) {
        return Ok(None);
    }
    verified
        .workflow_source_sha
        .filter(|_| verified.job_workflow.full_ref == input.job_workflow_ref)
        .ok_or(HookSourceError::Mismatch)
        .map(Some)
}

fn same_workflow_source(root: &str, job: &str) -> bool {
    let Some((root_path, root_reference)) = root.split_once('@') else {
        return false;
    };
    let Some((job_path, job_reference)) = job.split_once('@') else {
        return false;
    };
    let Some((root_owner, root_repository, root_file)) = workflow_identity(root_path) else {
        return false;
    };
    let Some((job_owner, job_repository, job_file)) = workflow_identity(job_path) else {
        return false;
    };
    root_owner.eq_ignore_ascii_case(job_owner)
        && root_repository.eq_ignore_ascii_case(job_repository)
        && root_file == job_file
        && root_reference == job_reference
}

fn workflow_identity(path: &str) -> Option<(&str, &str, &str)> {
    let mut parts = path.splitn(3, '/');
    let owner = parts.next()?;
    let repository = parts.next()?;
    let workflow = parts.next()?;
    if owner.is_empty() || repository.is_empty() || !workflow.starts_with(".github/workflows/") {
        return None;
    }
    Some((owner, repository, workflow))
}
