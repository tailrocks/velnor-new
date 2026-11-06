// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{
    RunnerGroup, ScaleSetByName, ScaleSetFound, get_runner_scale_set, list_runner_groups,
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

mod support;

use support::{
    create_connection_scale_set, default_group_id, map_host, map_repository, map_session,
    open_admin, revalidate_repository_scope, selector_valid,
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
    let (mut transport, admin) = open_admin(pat, &scope)?;
    let groups = list_runner_groups(&mut transport, admin.expose_token())
        .map_err(|error| map_session(error, &transport))?;
    let group_id = default_group_id(&groups)?;
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

    let set = match (
        lookup_action(record.state(), matches!(&found, ScaleSetFound::Found(_))),
        found,
    ) {
        (ScaleSetLookupAction::Create, ScaleSetFound::NotFound) => {
            create_connection_scale_set(
                pat,
                &scope,
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

fn lookup_action(state: ScaleSetCreateState, found: bool) -> ScaleSetLookupAction {
    match (state, found) {
        (ScaleSetCreateState::Prepared | ScaleSetCreateState::NoEffect, true) => {
            ScaleSetLookupAction::Adopt
        }
        (ScaleSetCreateState::Prepared, false) => ScaleSetLookupAction::Create,
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

pub fn product_runner_groups(
    pat: &str,
    scope: &VerifiedRepositoryScope,
) -> Result<Vec<RunnerGroup>, EnsureError> {
    if pat.is_empty() {
        return Err(EnsureError::Rejected);
    }
    revalidate_repository_scope(pat, scope)?;
    let (mut transport, admin) = open_admin(pat, scope)?;
    list_runner_groups(&mut transport, admin.expose_token())
        .map_err(|err| map_session(err, &transport))
}
