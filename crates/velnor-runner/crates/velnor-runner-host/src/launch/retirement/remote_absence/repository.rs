use velnor_runner_github::{
    Transport,
    workflow_run::{
        RepositoryTarget, VerifiedRepositoryScope, WorkflowRunError, get_repository_scope,
    },
};

use crate::{
    config::RepositoryConnection,
    journal::{ConnectionJournal, PreparedRetirementRemoteAbsence},
    scale_set::EnsureError,
};

pub(super) struct RepositoryParts<'a> {
    pub(super) owner: &'a str,
    pub(super) name: &'a str,
    pub(super) target: RepositoryTarget<'a>,
}

pub(super) fn validate_binding<'a>(
    scope: &ConnectionJournal,
    binding: &'a RepositoryConnection,
    prepared: &PreparedRetirementRemoteAbsence,
) -> Result<RepositoryParts<'a>, EnsureError> {
    let (owner, name) = split_repository(&binding.repository)?;
    let canonical = binding
        .canonical_identity_bytes()
        .map_err(|_| EnsureError::Rejected)?;
    if prepared.connection_id() != binding.connection_id.as_str()
        || scope.connection_id().as_str() != binding.connection_id.as_str()
        || prepared.binding_digest() != super::model::binding_digest(&canonical)
        || prepared.scale_set_id() <= 0
        || prepared.scale_set_name() != binding.scale_set.name()
        || prepared.scale_set_labels() != binding.scale_set.labels()
        || prepared.runner_group_id() <= 0
        || prepared.runner_id() <= 0
        || prepared.runner_name().is_empty()
    {
        return Err(EnsureError::Rejected);
    }
    Ok(RepositoryParts {
        owner,
        name,
        target: RepositoryTarget {
            id: binding.repository_id,
            owner,
            name,
        },
    })
}

pub(super) fn split_repository(repository: &str) -> Result<(&str, &str), EnsureError> {
    let (owner, name) = repository.split_once('/').ok_or(EnsureError::Rejected)?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err(EnsureError::Rejected);
    }
    Ok((owner, name))
}

pub(super) fn lookup_repository<T: Transport + ?Sized>(
    transport: &mut T,
    parts: &RepositoryParts<'_>,
    pat: &str,
) -> Result<VerifiedRepositoryScope, EnsureError> {
    let verified =
        get_repository_scope(transport, parts.target, pat).map_err(map_repository_error)?;
    if verified.id() != parts.target.id
        || verified.owner() != parts.owner
        || verified.name() != parts.name
    {
        return Err(EnsureError::Rejected);
    }
    Ok(verified)
}

pub(super) fn map_repository_error(error: WorkflowRunError) -> EnsureError {
    match error {
        WorkflowRunError::InvalidRequest | WorkflowRunError::RepositoryMismatch => {
            EnsureError::Rejected
        }
        WorkflowRunError::Uncertain => EnsureError::Uncertain,
        WorkflowRunError::HttpStatus(403) => EnsureError::Forbidden,
        WorkflowRunError::Malformed | WorkflowRunError::ResponseTooLarge => EnsureError::Malformed,
        WorkflowRunError::HttpStatus(status) => EnsureError::Unexpected {
            status,
            step: "repository-identity",
        },
    }
}
