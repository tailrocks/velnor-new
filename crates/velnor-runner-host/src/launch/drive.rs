// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::path::{Path, PathBuf};

use bollard::Docker;
use velnor_runner_github::Poll;

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::journal::{ConnectionJournal, Journal};
use crate::listen::Link;
use zeroize::Zeroize;

use super::{HostLane, ImageAuthority, SelectedEngineContext};
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

mod offer_authorization;

pub(crate) use offer_authorization::{
    PositiveOfferAuthorization, authorize_deferred_offers, authorize_positive_offer,
};
mod deferred_offer;
pub(crate) use deferred_offer::{DeferredAuthorizedOffer, DeferredAuthorizedOffers};
mod recovery;
pub(crate) use recovery::{AcquiredRecoveryContext, recover_acquired_scoped_offer};

pub(crate) struct Drive {
    pub(crate) set_id: i64,

    pub(crate) queue_path: String,

    pub(crate) queue_token: String,

    pub(crate) admin_token: String,

    pub(crate) docker_engine_id: Option<String>,

    pub(crate) owner: String,

    pub(crate) repo: String,

    pub(crate) pat: String,

    pub(crate) connection: Option<RepositoryConnection>,

    pub(crate) authority_policy: Option<ApprovedJobAuthorityPolicy>,

    pub(crate) host_journal: Option<Journal>,

    pub(crate) scoped_journal: Option<ConnectionJournal>,

    pub(crate) allocated_header: u32,

    pub(crate) source_authorized: bool,

    pub(crate) admission_state: Option<PathBuf>,
}

#[derive(Clone, Copy)]
pub(crate) struct Rest<'a> {
    pub(crate) owner: &'a str,

    pub(crate) repo: &'a str,

    pub(crate) pat: &'a str,

    pub(crate) connection: Option<&'a RepositoryConnection>,

    pub(crate) authority_policy: Option<&'a ApprovedJobAuthorityPolicy>,

    pub(crate) host_journal: Option<&'a Journal>,

    pub(crate) scoped_journal: Option<&'a ConnectionJournal>,

    pub(crate) allocated_header: u32,

    pub(crate) source_authorized: bool,

    pub(crate) admission_state: Option<&'a Path>,
}

impl fmt::Debug for Drive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Drive")
            .field("set_id", &self.set_id)
            .field("queue_path", &self.queue_path)
            .field("queue_token", &"[redacted]")
            .field("admin_token", &"[redacted]")
            .field("docker_engine_id", &self.docker_engine_id)
            .field("owner", &self.owner)
            .field("repo", &self.repo)
            .field(
                "connection_id",
                &self.connection.as_ref().map(|value| &value.connection_id),
            )
            .field("host_journal", &self.host_journal.is_some())
            .field("scoped_journal", &self.scoped_journal.is_some())
            .field("allocated_header", &self.allocated_header)
            .field("source_authorized", &self.source_authorized)
            .field("admission_state", &self.admission_state.is_some())
            .field("pat", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl Drive {
    pub(crate) fn from_rest(
        set_id: i64,
        queue_path: String,
        queue_token: String,
        admin_token: String,
        rest: Rest<'_>,
    ) -> Self {
        Self {
            set_id,
            queue_path,
            queue_token,
            admin_token,
            docker_engine_id: None,
            owner: rest.owner.to_owned(),
            repo: rest.repo.to_owned(),
            pat: rest.pat.to_owned(),
            connection: rest.connection.cloned(),
            authority_policy: rest.authority_policy.cloned(),
            host_journal: rest.host_journal.cloned(),
            scoped_journal: rest.scoped_journal.cloned(),
            allocated_header: rest.allocated_header,
            source_authorized: rest.source_authorized,
            admission_state: rest.admission_state.map(Path::to_path_buf),
        }
    }
}

pub(crate) struct ScopedAdmissionInput<'a> {
    pub(crate) link: &'a mut Link,
    pub(crate) queue_origin: Option<String>,
    pub(crate) queue_path: String,
    pub(crate) queue_token: String,
    pub(crate) admin_token: String,
    pub(crate) set_id: i64,
    pub(crate) polled: &'a Poll,
    pub(crate) docker: &'a Docker,
    pub(crate) connection: &'a RepositoryConnection,
    pub(crate) authority_policy: &'a ApprovedJobAuthorityPolicy,
    pub(crate) pat: &'a str,
    pub(crate) host_journal: &'a Journal,
    pub(crate) scoped_journal: &'a ConnectionJournal,
    pub(crate) image_authority: &'a ImageAuthority,
    pub(crate) selected_engine: &'a SelectedEngineContext,
    pub(crate) resource_budget: ResourceBudget,
    pub(crate) allocated_header: u32,
    pub(crate) admission_state: &'a Path,
}

pub(crate) async fn acquire_authorized_scoped_offer(
    input: ScopedAdmissionInput<'_>,
    authorized: &DeferredAuthorizedOffer,
) -> Result<super::steps::ScopedProvisioningOutcome, EnsureError> {
    if !authorized
        .matches_current_envelope(input.polled, input.scoped_journal, input.set_id)
        .await
        || authorized.offer().connection_id() != &input.connection.connection_id
    {
        return Err(EnsureError::Uncertain);
    }
    let offer = authorized.offer();
    acquire_scoped_offer_inner(input, offer).await
}

async fn acquire_scoped_offer_inner(
    mut input: ScopedAdmissionInput<'_>,
    authorized: &crate::journal::AuthorizedOffer,
) -> Result<super::steps::ScopedProvisioningOutcome, EnsureError> {
    if input.allocated_header == 0 {
        return Err(EnsureError::AdmissionClosed);
    }
    let (owner, repo) = input
        .connection
        .repository
        .split_once('/')
        .ok_or(EnsureError::AdmissionClosed)?;
    let rest = Rest {
        owner,
        repo,
        pat: input.pat,
        connection: Some(input.connection),
        authority_policy: Some(input.authority_policy),
        host_journal: Some(input.host_journal),
        scoped_journal: Some(input.scoped_journal),
        allocated_header: input.allocated_header,
        source_authorized: true,
        admission_state: Some(input.admission_state),
    };
    let (_batch, request_id) =
        super::steps::assignment(input.polled)?.ok_or(EnsureError::Uncertain)?;
    let binding = input
        .scoped_journal
        .source_authorization_binding()
        .await
        .map_err(|_| EnsureError::Uncertain)?;
    if authorized.connection_id() != input.scoped_journal.connection_id()
        || authorized.connection_id() != &input.connection.connection_id
        || authorized.scale_set_id() != input.set_id
        || authorized.request_id() != request_id
        || authorized.binding_digest() != binding.binding_digest()
    {
        return Err(EnsureError::Uncertain);
    }

    let Some(permit) = super::resource_probe::start_permit(
        input.docker,
        input.host_journal,
        input.resource_budget,
        input.image_authority,
        input.selected_engine.endpoint(),
    )
    .await
    else {
        return Ok(super::steps::ScopedProvisioningOutcome::NoStart);
    };
    let mut drive = Drive::from_rest(
        input.set_id,
        input.queue_path,
        input.queue_token,
        input.admin_token,
        Rest {
            source_authorized: true,
            ..rest
        },
    );
    drive.docker_engine_id = Some(input.selected_engine.engine_id().to_owned());
    let admin = input.link.base().to_owned();
    let mut lane = HostLane::new(input.link, admin, input.queue_origin.take());
    super::steps::acquire_scoped(
        &mut lane,
        &drive,
        input.polled,
        permit,
        input.selected_engine,
    )
    .await
}

impl Drop for Drive {
    fn drop(&mut self) {
        self.queue_token.zeroize();
        self.admin_token.zeroize();
        self.pat.zeroize();
    }
}

pub(crate) trait Lane {
    fn on_admin(&mut self) -> Result<(), EnsureError>;

    fn on_queue(&mut self) -> Result<(), EnsureError>;
}
