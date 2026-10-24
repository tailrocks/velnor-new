use std::collections::{BTreeMap, BTreeSet, VecDeque};

use velnor_runner_github::workflow_run::{
    ComparedHookSourceFacts, RepositoryTarget, WorkflowRun, get_workflow_content,
};

use crate::config::{ApprovedJobAuthorityPolicy, ApprovedWorkflowSource, RepositoryConnection};
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::execution_authorization::NativeStartedSourceContext;

use super::super::{ExecutionSourceRequest, VerifiedWorkflowIdentity};

#[derive(Clone, Copy)]
pub(super) struct WorkflowVerification<'a> {
    pub(super) connection: &'a RepositoryConnection,
    pub(super) policy: &'a ApprovedJobAuthorityPolicy,
    pub(super) request: &'a ExecutionSourceRequest,
    pub(super) native: &'a NativeStartedSourceContext,
    pub(super) owner: &'a str,
    pub(super) repository: &'a str,
    pub(super) target: RepositoryTarget<'a>,
    pub(super) run: &'a WorkflowRun,
    pub(super) compared: &'a ComparedHookSourceFacts,
}

pub(super) struct VerifiedWorkflowClosure {
    pub(super) root_path: String,
    pub(super) leaf_path: String,
    pub(super) identities: Vec<VerifiedWorkflowIdentity>,
}

pub(super) fn verify_workflow_closure(
    input: WorkflowVerification<'_>,
    transport: &mut HttpsTransport,
    token: &str,
) -> Result<VerifiedWorkflowClosure, HostError> {
    let WorkflowVerification {
        connection,
        policy,
        request,
        native,
        owner,
        repository,
        target,
        run,
        compared,
    } = input;
    let workflow_inventory = policy
        .workflow_sources()
        .map(|workflow| (workflow.path(), workflow))
        .collect::<BTreeMap<_, _>>();
    let root_path = workflow_path(request.workflow_ref(), owner, repository)?;
    let (leaf_ref, leaf_source_sha) = compared.job_workflow();
    if native.job_workflow_ref() != leaf_ref {
        return Err(HostError::Identity);
    }
    let leaf_path = workflow_path(leaf_ref, owner, repository)?;
    let root_sha = request.workflow_sha();
    let leaf_sha = if leaf_path == root_path {
        if native
            .workflow_source_sha()
            .is_some_and(|sha| sha != root_sha)
            || leaf_source_sha.is_some()
        {
            return Err(HostError::Identity);
        }
        root_sha
    } else {
        let source_sha = leaf_source_sha.ok_or(HostError::Identity)?;
        if native.workflow_source_sha() != Some(source_sha) {
            return Err(HostError::Identity);
        }
        source_sha
    };
    let source_refs = workflow_sources(run, owner, repository, &root_path, root_sha)?;
    if source_refs.get(&leaf_path).map(String::as_str) != Some(leaf_sha) {
        return Err(HostError::Identity);
    }
    ensure_approved_closure(&workflow_inventory, &root_path, &source_refs, connection)?;
    let identities = fetch_workflow_identities(
        connection,
        &workflow_inventory,
        &source_refs,
        target,
        transport,
        token,
    )?;
    Ok(VerifiedWorkflowClosure {
        root_path,
        leaf_path,
        identities,
    })
}

fn fetch_workflow_identities(
    connection: &RepositoryConnection,
    inventory: &BTreeMap<&str, ApprovedWorkflowSource<'_>>,
    source_refs: &BTreeMap<String, String>,
    target: RepositoryTarget<'_>,
    transport: &mut HttpsTransport,
    token: &str,
) -> Result<Vec<VerifiedWorkflowIdentity>, HostError> {
    let mut identities = Vec::with_capacity(source_refs.len());
    for (path, commit_sha) in source_refs {
        let approved = inventory.get(path.as_str()).ok_or(HostError::Identity)?;
        let content = get_workflow_content(transport, target, commit_sha, path, token)
            .map_err(|_| HostError::Identity)?;
        if content.repository_id() != connection.repository_id
            || content.path() != path
            || content.commit_sha() != commit_sha
            || content.content_sha256() != approved.content_sha256()
        {
            return Err(HostError::Identity);
        }
        identities.push(VerifiedWorkflowIdentity {
            repository_id: u64::try_from(connection.repository_id)
                .map_err(|_| HostError::Identity)?,
            path: path.clone(),
            commit_sha: content.commit_sha().to_owned(),
            tree_sha: content.tree_sha().to_owned(),
            blob_sha: content.blob_sha().to_owned(),
            content_sha256: content.content_sha256().to_owned(),
        });
    }
    Ok(identities)
}

fn workflow_sources(
    run: &WorkflowRun,
    owner: &str,
    repository: &str,
    root_path: &str,
    root_sha: &str,
) -> Result<BTreeMap<String, String>, HostError> {
    let mut sources = BTreeMap::new();
    insert_source_ref(&mut sources, root_path.to_owned(), root_sha.to_owned())?;
    for referenced in &run.referenced_workflows {
        let path = workflow_path(&referenced.path, owner, repository)?;
        insert_source_ref(&mut sources, path, referenced.sha.clone())?;
    }
    Ok(sources)
}

pub(super) fn insert_source_ref(
    sources: &mut BTreeMap<String, String>,
    path: String,
    commit_sha: String,
) -> Result<(), HostError> {
    if sources.insert(path, commit_sha).is_some() {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}

pub(super) fn root_source_commit_matches(request: &ExecutionSourceRequest) -> bool {
    request.workflow_sha() == request.tested_sha()
}

fn workflow_path(value: &str, owner: &str, repository: &str) -> Result<String, HostError> {
    let path = value.split_once('@').map_or(value, |(path, _)| path);
    let path = path.strip_prefix("./").unwrap_or(path);
    let prefix = format!("{owner}/{repository}/");
    let path = path.strip_prefix(&prefix).unwrap_or(path);
    if path.starts_with(".github/workflows/")
        && !path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        Ok(path.to_owned())
    } else {
        Err(HostError::Identity)
    }
}

fn ensure_approved_closure(
    inventory: &BTreeMap<&str, ApprovedWorkflowSource<'_>>,
    root_path: &str,
    source_refs: &BTreeMap<String, String>,
    connection: &RepositoryConnection,
) -> Result<(), HostError> {
    let mut pending = VecDeque::from([root_path.to_owned()]);
    let mut reachable = BTreeSet::new();
    while let Some(path) = pending.pop_front() {
        if !reachable.insert(path.clone()) {
            continue;
        }
        let workflow = inventory.get(path.as_str()).ok_or(HostError::Identity)?;
        if workflow.repository_id() != connection.repository_id {
            return Err(HostError::Identity);
        }
        for call in workflow.calls() {
            if call.repository_id() != connection.repository_id
                || inventory
                    .get(call.path())
                    .is_none_or(|target| target.content_sha256() != call.content_sha256())
            {
                return Err(HostError::Identity);
            }
            pending.push_back(call.path().to_owned());
        }
    }
    if !source_refs_match_approved_closure(&reachable, source_refs) {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn source_refs_match_approved_closure(
    reachable: &BTreeSet<String>,
    source_refs: &BTreeMap<String, String>,
) -> bool {
    let observed = source_refs.keys().cloned().collect::<BTreeSet<_>>();
    &observed == reachable
}
