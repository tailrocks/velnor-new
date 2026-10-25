//! Get or create the selector from host configuration.

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
#[cfg(test)]
use support::create_labels;
use support::{
    create_connection_scale_set, default_group_id, map_host, map_repository, map_session,
    open_admin, revalidate_repository_scope, selector_valid,
};

const GITHUB_API: &str = "https://api.github.com";

/// Identifiers safe to print. No token and no service URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsuredSet {
    /// Positive scale-set id.
    pub id: i64,
    /// Whether this controller created or adopted the remote Scale Set.
    pub ownership: SetOwnership,
    /// Scale-set name.
    pub name: String,
    /// Repository default group id returned by the Actions service.
    pub runner_group_id: i64,
    /// Registration invariant.
    pub disable_update: bool,
    /// Label names in service order.
    pub labels: Vec<String>,
}

/// Exact Scale Set and the admin connection used to verify or create it.
///
/// The link stays scoped to this one configured repository connection.
pub(crate) struct ConnectionSetup {
    /// Verified Scale Set identity and ownership.
    pub(crate) set: EnsuredSet,
    /// Ordered Runner Groups returned by the same setup read.
    pub(crate) groups: Vec<RunnerGroup>,
    /// Admin link minted for the same verified repository scope.
    pub(crate) link: Link,
    /// Journal view validated from the exact configured binding.
    pub(crate) scoped: ConnectionJournal,
}

/// Failure that does not include a credential or a response body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnsureError {
    /// The endpoint was not `https`.
    #[error("bad endpoint")]
    Endpoint,
    /// The service rejected the registration invariant.
    #[error("registration rejected")]
    Rejected,
    /// HTTP 403.
    #[error("forbidden")]
    Forbidden,
    /// The body did not match the pinned DTO.
    #[error("malformed message")]
    Malformed,
    /// Timeout or reset. The create may have happened.
    #[error("effect uncertain")]
    Uncertain,
    /// Another session holds the set.
    #[error("session conflict")]
    Conflict,
    /// The offline runner name was removed. Mint again. Do not acknowledge yet.
    #[error("runner name cleared")]
    NameCleared,
    /// This offline runner id was already removed. Do not delete it again.
    /// Do not acknowledge, and do not keep a slot.
    #[error("runner name unchanged")]
    NameSteady,
    /// Job source data or policy did not authorize admission.
    #[error("job source is blocked: {reason}")]
    SourceBlocked {
        /// Stable reason without repository source values.
        reason: &'static str,
    },
    /// A drain marker or config change closed scoped admission.
    #[error("admission is closed")]
    AdmissionClosed,
    /// A finished status was not the one the call expects.
    #[error("unexpected status {status} at {step}")]
    Unexpected {
        /// HTTP status. Zero means the transport failed before a status.
        status: u16,
        /// Which registration call failed.
        step: &'static str,
    },
}

/// Get or durably create the exact Scale Set for one repository connection.
///
/// The create claim is committed before the POST. An ambiguous response stays
/// uncertain and cannot be adopted by name or retried automatically.
/// The repository ID, owner/name, and private visibility are verified through
/// GitHub before registration and immediately before creation. GitHub's
/// registration API is name-scoped and does not accept an immutable repository
/// ID. Its admin-connection response also contains no documented immutable
/// repository ID. Therefore the metadata checks do not prove that the admin
/// token remains bound to the configured repository across a rename/transfer
/// ABA. Operators must keep the owner/name mapping stable and prevent rename,
/// transfer, or name reuse for the full admin-connection and Scale Set setup
/// operation. The API provides no atomic check that can replace this condition.
///
/// # Errors
///
/// Returns [`EnsureError`] if repository registration, the journal, or remote
/// Scale Set identity cannot be verified.
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

/// Runner groups visible to this repository's Actions connection.
///
/// # Errors
///
/// Returns [`EnsureError`] when the endpoint, token, or group list is refused.
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

#[cfg(test)]
mod tests;
