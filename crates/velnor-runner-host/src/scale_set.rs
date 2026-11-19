// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{
    RunnerGroup, ScaleSetById, ScaleSetByName, ScaleSetFound, Transport, get_runner_scale_set,
    get_runner_scale_set_by_id, list_runner_groups, list_scale_set_runners,
    workflow_run::{RepositoryTarget, VerifiedRepositoryScope, get_repository_scope},
};

use crate::config::RepositoryConnection;
use crate::connect::SetOwnership;
use crate::https::HttpsTransport;
use crate::journal::{
    ConnectionJournal, ExistingScaleSetObservation, Journal, ScaleSetCreateRequest,
    ScaleSetCreateState,
};
use crate::listen::{Link, link_from_admin};
use delete::{finish_recovered_delete, recover_intended_delete};

pub(crate) mod delete;
mod support;

mod removal;

pub(crate) use removal::{RemovedRunnerProver, RunnerProver};

use support::{
    CreateIds, create_connection_scale_set, map_host, map_repository, map_session, open_admin,
    revalidate_repository_scope, selected_group_id, selector_valid,
};

const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsuredSet {
    pub id: i64,

    pub ownership: SetOwnership,

    pub name: String,

    pub runner_group_id: i64,

    pub disable_update: bool,

    pub labels: Vec<String>,
}

pub(crate) struct ConnectionSetup {
    pub(crate) set: EnsuredSet,

    pub(crate) groups: Vec<RunnerGroup>,

    pub(crate) link: Link,

    pub(crate) scoped: ConnectionJournal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnsureError {
    #[error("bad endpoint")]
    Endpoint,

    #[error("registration rejected")]
    Rejected,

    #[error("forbidden")]
    Forbidden,

    #[error("malformed message")]
    Malformed,

    #[error("unauthorized")]
    Unauthorized,

    #[error("effect uncertain")]
    Uncertain,

    #[error("session conflict")]
    Conflict,

    #[error("runner name cleared")]
    NameCleared,

    #[error("runner name unchanged")]
    NameSteady,

    #[error("job source is blocked: {reason}")]
    SourceBlocked { reason: &'static str },

    #[error("admission is closed")]
    AdmissionClosed,

    #[error("unexpected status {status} at {step}")]
    Unexpected { status: u16, step: &'static str },
}

pub(crate) async fn ensure_connection_scale_set(
    pat: &str,
    binding: &RepositoryConnection,
    journal: &Journal,
) -> Result<ConnectionSetup, EnsureError> {
    let scoped = journal.connection_scope(binding).await.map_err(map_host)?;
    let (owner, repo) = repository_parts(&binding.repository)?;
    if pat.is_empty() || !selector_valid(&binding.scale_set) {
        return Err(EnsureError::Rejected);
    }
    let mut identity_transport = HttpsTransport::new(GITHUB_API).map_err(map_host)?;
    let scope = get_repository_scope(
        &mut identity_transport,
        RepositoryTarget {
            id: binding.repository_id,
            owner,
            name: repo,
        },
        pat,
    )
    .map_err(map_repository)?;
    let (mut transport, admin) = open_admin(pat, &scope, binding)?;
    let groups = list_runner_groups(&mut transport, admin.expose_token())
        .map_err(|error| map_session(error, &transport))?;
    let group_id = selected_group_id(binding, &groups)?;
    let recovered = recover_intended_delete(&mut transport, admin.expose_token(), &scoped).await;
    finish_recovered_delete(journal, binding, recovered.map_err(map_host)?)
        .await
        .map_err(map_host)?;
    let request = ScaleSetCreateRequest::new(binding, group_id).map_err(map_host)?;
    let record = scoped
        .prepare_scale_set_create(&request)
        .await
        .map_err(map_host)?;
    let found = get_runner_scale_set(
        &mut transport,
        &ScaleSetByName {
            runner_group_id: group_id,
            name: request.selector_name(),
            expected_labels: request.selector_labels(),
            admin_token: admin.expose_token(),
        },
    )
    .map_err(|error| map_session(error, &transport))?;

    let exists = matches!(&found, ScaleSetFound::Found(_));
    let organization = binding.organization.is_some();
    let set = match (lookup_action(record.state(), exists, organization), found) {
        (ScaleSetLookupAction::Create, ScaleSetFound::NotFound) => {
            create_connection_scale_set(
                CreateIds {
                    pat,
                    scope: &scope,
                    adopt_on_conflict: !organization,
                },
                &mut transport,
                admin.expose_token(),
                &scoped,
                &request,
                &record,
            )
            .await?
        }
        (
            ScaleSetLookupAction::Adopt | ScaleSetLookupAction::Verify,
            ScaleSetFound::Found(view),
        ) => {
            revalidate_repository_scope(pat, &scope)?;
            let observation = observed_scale_set(&request, view)?;
            let adopted = scoped
                .adopt_scale_set(&request, observation)
                .await
                .map_err(map_host)?;
            ensured_from_record(&adopted)?
        }
        (_, _) => return Err(EnsureError::Uncertain),
    };
    let link = link_from_admin(transport.inner, admin, binding, &scope)?;
    Ok(ConnectionSetup {
        set,
        groups,
        link,
        scoped,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScaleSetLookupAction {
    Adopt,
    Create,
    Verify,
    Hold,
}

fn lookup_action(
    state: ScaleSetCreateState,
    found: bool,
    organization: bool,
) -> ScaleSetLookupAction {
    match (state, found) {
        (
            ScaleSetCreateState::Prepared
            | ScaleSetCreateState::Requested
            | ScaleSetCreateState::Uncertain
            | ScaleSetCreateState::NoEffect,
            true,
        ) if !organization => ScaleSetLookupAction::Adopt,
        (ScaleSetCreateState::Prepared | ScaleSetCreateState::NoEffect, false) => {
            ScaleSetLookupAction::Create
        }
        (ScaleSetCreateState::Created | ScaleSetCreateState::Adopted, true) => {
            ScaleSetLookupAction::Verify
        }
        _ => ScaleSetLookupAction::Hold,
    }
}

fn observed_scale_set(
    request: &ScaleSetCreateRequest,
    view: velnor_runner_github::ScaleSetView,
) -> Result<ExistingScaleSetObservation, EnsureError> {
    let labels = view
        .labels
        .into_iter()
        .map(|label| label.name)
        .collect::<Vec<_>>();
    ExistingScaleSetObservation::verified(
        request,
        view.id,
        &view.name,
        &labels,
        view.runner_setting.disable_update,
    )
    .map_err(map_host)
}

fn ensured_from_record(
    record: &crate::journal::ScaleSetCreateRecord,
) -> Result<EnsuredSet, EnsureError> {
    let receipt = record.receipt().ok_or(EnsureError::Uncertain)?;
    Ok(EnsuredSet {
        id: receipt.id,
        ownership: receipt.ownership,
        name: receipt.name.clone(),
        runner_group_id: record.request().runner_group_id(),
        disable_update: receipt.disable_update,
        labels: receipt.labels.clone(),
    })
}

fn repository_parts(repository: &str) -> Result<(&str, &str), EnsureError> {
    let Some((owner, repo)) = repository.split_once('/') else {
        return Err(EnsureError::Rejected);
    };
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return Err(EnsureError::Rejected);
    }
    Ok((owner, repo))
}

/// Opens the admin session of `binding`'s repository: the verified scope, then an admin
/// connection. Shared by every observation and removal that acts for one connection.
fn open_binding_admin(
    pat: &str,
    binding: &RepositoryConnection,
) -> Result<(support::Recording, velnor_runner_github::AdminConnection), EnsureError> {
    let (repo_owner, repo_name) = repository_parts(&binding.repository)?;
    if pat.is_empty() || !selector_valid(&binding.scale_set) {
        return Err(EnsureError::Rejected);
    }
    let mut identity_transport = HttpsTransport::new(GITHUB_API).map_err(map_host)?;
    let scope = get_repository_scope(
        &mut identity_transport,
        RepositoryTarget {
            id: binding.repository_id,
            owner: repo_owner,
            name: repo_name,
        },
        pat,
    )
    .map_err(map_repository)?;
    open_admin(pat, &scope, binding)
}

/// Whether `binding`'s selector owns `scale_set_id`.
fn binding_owns_set<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    binding: &RepositoryConnection,
    scale_set_id: i64,
) -> Result<bool, EnsureError> {
    let request = ScaleSetCreateRequest::new(binding, 1).map_err(map_host)?;
    let owned = get_runner_scale_set_by_id(
        transport,
        &ScaleSetById {
            scale_set_id,
            name: request.selector_name(),
            expected_labels: request.selector_labels(),
            admin_token,
        },
    );
    Ok(matches!(owned, Ok(view) if view.id == scale_set_id))
}

/// Lists the runners GitHub reports for `scale_set_id`, or `None` when this
/// connection does not own that scale set (so it cannot be observed here).
pub(crate) fn observe_scale_set_runners(
    pat: &str,
    binding: &RepositoryConnection,
    scale_set_id: i64,
) -> Result<Option<Vec<String>>, EnsureError> {
    let (mut transport, admin) = open_binding_admin(pat, binding)?;
    if !binding_owns_set(&mut transport, admin.expose_token(), binding, scale_set_id)? {
        return Ok(None);
    }
    list_runner_ids(&mut transport, admin.expose_token(), scale_set_id).map(Some)
}

pub(crate) fn list_runner_ids<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    scale_set_id: i64,
) -> Result<Vec<String>, EnsureError> {
    let runners = list_scale_set_runners(transport, scale_set_id, admin_token)
        .map_err(|_| EnsureError::Uncertain)?;
    Ok(runners
        .into_iter()
        .map(|runner| runner.id.to_string())
        .collect())
}

pub fn product_runner_groups(
    pat: &str,
    scope: &VerifiedRepositoryScope,
    binding: &RepositoryConnection,
) -> Result<Vec<RunnerGroup>, EnsureError> {
    if pat.is_empty() {
        return Err(EnsureError::Rejected);
    }
    revalidate_repository_scope(pat, scope)?;
    let (mut transport, admin) = open_admin(pat, scope, binding)?;
    list_runner_groups(&mut transport, admin.expose_token())
        .map_err(|err| map_session(err, &transport))
}

#[cfg(test)]
mod tests;
