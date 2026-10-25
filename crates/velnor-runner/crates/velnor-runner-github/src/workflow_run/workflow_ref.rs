//! Typed workflow refs from authenticated Scale Set job messages.

use super::WorkflowRun;

/// Ref kind reported by the authenticated job workflow ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowRefKind {
    /// A branch ref.
    Branch,
    /// A tag ref.
    Tag,
    /// A pull request merge ref.
    PullRequest,
    /// A merge queue ref.
    MergeQueue,
    /// A workflow pinned to a commit.
    Commit,
    /// Another full Git ref.
    Other,
}

/// Workflow file ref matched with the GitHub workflow run record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedJobWorkflowRef {
    /// Exact `jobWorkflowRef` value from the job message.
    pub full_ref: String,
    /// Repository owner named by the workflow ref.
    pub owner_name: String,
    /// Repository named by the workflow ref.
    pub repository_name: String,
    /// Workflow file path within the repository.
    pub workflow_path: String,
    /// Full Git ref, such as `refs/heads/main`.
    pub reference: String,
    /// Ref kind from the full Git ref.
    pub kind: WorkflowRefKind,
}

struct ParsedWorkflowRef<'a> {
    owner_name: &'a str,
    repository_name: &'a str,
    workflow_path: &'a str,
    reference: &'a str,
    kind: WorkflowRefKind,
}

/// Match a job workflow ref to the run's root workflow or a called workflow.
pub(super) fn workflow_source<'a>(
    run: &'a WorkflowRun,
    full_ref: &str,
) -> Option<(VerifiedJobWorkflowRef, Option<&'a str>)> {
    let parsed = parse(full_ref)?;
    if workflow_path_matches(
        &parsed,
        &run.path,
        Some((&run.repository.owner.login, &run.repository.name)),
        None,
        None,
    ) {
        return Some((verified(full_ref, &parsed), None));
    }
    run.referenced_workflows
        .iter()
        .find(|workflow| {
            workflow_path_matches(
                &parsed,
                &workflow.path,
                None,
                workflow.reference.as_deref(),
                Some(&workflow.sha),
            )
        })
        .map(|workflow| (verified(full_ref, &parsed), Some(workflow.sha.as_str())))
}

fn parse(value: &str) -> Option<ParsedWorkflowRef<'_>> {
    let (path, reference) = value.split_once('@')?;
    if invalid_text(reference) || (!reference.starts_with("refs/") && !valid_commit_sha(reference))
    {
        return None;
    }
    let mut parts = path.splitn(3, '/');
    let owner_name = parts.next()?;
    let repository_name = parts.next()?;
    let workflow_path = parts.next()?;
    if !valid_repository_name(owner_name)
        || !valid_repository_name(repository_name)
        || !valid_workflow_path(workflow_path)
    {
        return None;
    }
    Some(ParsedWorkflowRef {
        owner_name,
        repository_name,
        workflow_path,
        reference,
        kind: classify(reference),
    })
}

fn workflow_path_matches(
    job_ref: &ParsedWorkflowRef<'_>,
    api_ref: &str,
    fallback_repository: Option<(&str, &str)>,
    explicit_reference: Option<&str>,
    source_sha: Option<&str>,
) -> bool {
    let (api_path, api_revision) = api_ref
        .split_once('@')
        .map_or((api_ref, None), |(path, revision)| (path, Some(revision)));
    let Some((api_owner, api_repository, api_workflow_path)) =
        api_identity(api_path, fallback_repository)
    else {
        return false;
    };
    let identity_matches = api_owner.eq_ignore_ascii_case(job_ref.owner_name)
        && api_repository.eq_ignore_ascii_case(job_ref.repository_name)
        && api_workflow_path == job_ref.workflow_path;
    identity_matches
        && ref_matches_api_path(
            job_ref,
            api_revision,
            explicit_reference,
            source_sha,
            fallback_repository.is_some(),
        )
}

fn ref_matches_api_path(
    job_ref: &ParsedWorkflowRef<'_>,
    api_revision: Option<&str>,
    explicit_reference: Option<&str>,
    source_sha: Option<&str>,
    is_root_workflow: bool,
) -> bool {
    match (api_revision, explicit_reference) {
        (Some(revision), Some(reference)) => {
            reference_matches(job_ref.reference, reference)
                && path_revision_matches(job_ref.reference, revision, reference)
        }
        (Some(revision), None) => reference_matches(job_ref.reference, revision),
        (None, Some(reference)) => reference_matches(job_ref.reference, reference),
        (None, None) if is_root_workflow => true,
        (None, None) => {
            matches!(job_ref.kind, WorkflowRefKind::Commit) && source_sha == Some(job_ref.reference)
        }
    }
}

fn api_identity<'a>(
    path: &'a str,
    fallback_repository: Option<(&'a str, &'a str)>,
) -> Option<(&'a str, &'a str, &'a str)> {
    let without_prefix = path.strip_prefix("./").unwrap_or(path);
    if let Some(offset) = without_prefix.find(".github/workflows/") {
        let prefix = &without_prefix[..offset];
        let workflow_path = &without_prefix[offset..];
        if prefix.is_empty() {
            let (owner, repository) = fallback_repository?;
            return Some((owner, repository, workflow_path));
        }
        let prefix = prefix.strip_suffix('/')?;
        let mut parts = prefix.split('/');
        let owner = parts.next()?;
        let repository = parts.next()?;
        if parts.next().is_some()
            || !valid_repository_name(owner)
            || !valid_repository_name(repository)
        {
            return None;
        }
        return Some((owner, repository, workflow_path));
    }
    None
}

fn reference_matches(job_reference: &str, api_reference: &str) -> bool {
    if api_reference.starts_with("refs/") {
        return job_reference == api_reference;
    }
    if matches!(classify(job_reference), WorkflowRefKind::Branch) {
        return job_reference
            .strip_prefix("refs/heads/")
            .is_some_and(|reference| reference == api_reference);
    }
    matches!(classify(job_reference), WorkflowRefKind::Commit) && job_reference == api_reference
}

fn path_revision_matches(
    job_reference: &str,
    api_revision: &str,
    explicit_reference: &str,
) -> bool {
    if api_revision.starts_with("refs/") {
        return job_reference == api_revision;
    }
    if matches!(classify(job_reference), WorkflowRefKind::Commit) {
        return job_reference == api_revision && job_reference == explicit_reference;
    }
    ref_name(explicit_reference).is_some_and(|name| name == api_revision)
}

fn ref_name(reference: &str) -> Option<&str> {
    reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/tags/"))
        .or_else(|| reference.strip_prefix("refs/"))
}

fn classify(reference: &str) -> WorkflowRefKind {
    if valid_commit_sha(reference) {
        WorkflowRefKind::Commit
    } else if reference
        .strip_prefix("refs/heads/")
        .is_some_and(|value| !value.is_empty())
    {
        WorkflowRefKind::Branch
    } else if reference
        .strip_prefix("refs/tags/")
        .is_some_and(|value| !value.is_empty())
    {
        WorkflowRefKind::Tag
    } else if reference
        .strip_prefix("refs/pull/")
        .is_some_and(valid_pull_ref)
    {
        WorkflowRefKind::PullRequest
    } else if reference
        .strip_prefix("refs/merge-queue/")
        .is_some_and(|value| !value.is_empty())
    {
        WorkflowRefKind::MergeQueue
    } else {
        WorkflowRefKind::Other
    }
}

fn valid_pull_ref(value: &str) -> bool {
    let Some((number, suffix)) = value.split_once('/') else {
        return false;
    };
    number.parse::<u64>().is_ok_and(|number| number > 0) && suffix == "merge"
}

fn verified(value: &str, parsed: &ParsedWorkflowRef<'_>) -> VerifiedJobWorkflowRef {
    VerifiedJobWorkflowRef {
        full_ref: value.to_owned(),
        owner_name: parsed.owner_name.to_owned(),
        repository_name: parsed.repository_name.to_owned(),
        workflow_path: parsed.workflow_path.to_owned(),
        reference: parsed.reference.to_owned(),
        kind: parsed.kind,
    }
}

fn valid_repository_name(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_workflow_path(path: &str) -> bool {
    let Some(suffix) = path.strip_prefix(".github/workflows/") else {
        return false;
    };
    !suffix.is_empty()
        && !invalid_text(suffix)
        && suffix
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}

fn invalid_text(value: &str) -> bool {
    value.trim().is_empty() || value.chars().any(char::is_control)
}

fn valid_commit_sha(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}
