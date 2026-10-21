//! Launch call context. Tokens stay out of `Debug` and are zeroized on drop.

use std::fmt;
use std::path::{Path, PathBuf};

use bollard::Docker;
use velnor_runner_github::Poll;

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::journal::{ConnectionJournal, Journal};
use crate::listen::Link;
use crate::worker::guest_image::GuestImageFile;
use zeroize::Zeroize;

use super::{HostLane, ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages};
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

mod offer_authorization;
#[cfg(test)]
pub(crate) use offer_authorization::authorize_positive_offer_with;
pub(crate) use offer_authorization::{
    PositiveOfferAuthorization, authorize_deferred_offers, authorize_positive_offer,
};
mod deferred_offer;
pub(crate) use deferred_offer::{DeferredAuthorizedOffer, DeferredAuthorizedOffers};
mod recovery;
pub(crate) use recovery::{AcquiredRecoveryContext, recover_acquired_scoped_offer};
#[cfg(test)]
pub(crate) use recovery::{
    recover_acquired_scoped_offer_with_test_observation,
    recover_acquired_scoped_offer_with_test_provider,
};

/// Call context. Tokens are redacted in `Debug` and zeroized on drop.
pub(crate) struct Drive {
    /// Scale-set id.
    pub(crate) set_id: i64,
    /// Queue path. No host.
    pub(crate) queue_path: String,
    /// Queue bearer.
    pub(crate) queue_token: String,
    /// Admin bearer.
    pub(crate) admin_token: String,
    /// Selected Docker engine identity, captured before a launch row is prepared.
    pub(crate) docker_engine_id: Option<String>,
    /// Repository owner. Empty skips runner-directory calls.
    pub(crate) owner: String,
    /// Repository name. Empty skips runner-directory calls.
    pub(crate) repo: String,
    /// REST credential for `api.github.com`. Not a queue token.
    pub(crate) pat: String,
    /// Repository identity and source policy for scoped admission.
    pub(crate) connection: Option<RepositoryConnection>,
    /// Validated source policy for this connection.
    pub(crate) authority_policy: Option<ApprovedJobAuthorityPolicy>,
    /// Shared capacity journal used by selected-engine probes.
    pub(crate) host_journal: Option<Journal>,
    /// Per-connection journal used by launch and completion records.
    pub(crate) scoped_journal: Option<ConnectionJournal>,
    /// This connection's share of the host poll header.
    pub(crate) allocated_header: u32,
    /// Source checks completed for the current polled job.
    pub(crate) source_authorized: bool,
    /// State path used to fence scoped admission against drain and config edits.
    pub(crate) admission_state: Option<PathBuf>,
}

/// Owner, repository, and REST credential for one launch.
#[derive(Clone, Copy)]
pub(crate) struct Rest<'a> {
    /// Repository owner.
    pub(crate) owner: &'a str,
    /// Repository name.
    pub(crate) repo: &'a str,
    /// PAT. Not logged.
    pub(crate) pat: &'a str,
    /// Validated runner and `DinD` limits for new worker pairs.
    pub(crate) resource_budget: Option<ResourceBudget>,
    /// Static Docker CPU and memory totals can fit a configured worker pair.
    pub(crate) static_capacity: bool,
    /// Fresh guest metrics required before any acquire, JIT, or worker start.
    pub(crate) guest_admission: GuestAdmission,
    /// Repository connection that authorizes job source metadata.
    pub(crate) connection: Option<&'a RepositoryConnection>,
    /// Validated source policy for this connection.
    pub(crate) authority_policy: Option<&'a ApprovedJobAuthorityPolicy>,
    /// Shared host journal for capacity and selected-engine probes.
    pub(crate) host_journal: Option<&'a Journal>,
    /// Dedicated host credential for verified release and probe access.
    pub(crate) release_read_secret: Option<&'a ReleaseReadSecret>,
    /// Scoped journal for connection-owned records.
    pub(crate) scoped_journal: Option<&'a ConnectionJournal>,
    /// Header capacity allocated by the shared host controller.
    pub(crate) allocated_header: u32,
    /// Source check completed for the current poll.
    pub(crate) source_authorized: bool,
    /// Host state path for the scoped admission fence.
    pub(crate) admission_state: Option<&'a Path>,
    /// Verified worker images bound to the selected Docker daemon.
    pub(crate) worker_images: Option<&'a VerifiedWorkerImages>,
    /// Selected engine context paired with the verified image bundle.
    pub(crate) selected_engine: Option<&'a SelectedEngineContext>,
    /// Immutable per-connection guest image metadata artifact.
    pub(crate) guest_image: Option<&'a GuestImageFile>,
}

impl Rest<'_> {
    #[must_use]
    pub(crate) const fn permits_start(self) -> bool {
        self.static_capacity && (self.scoped_journal.is_none() || self.allocated_header > 0)
    }
}

/// Whether this job-start attempt has a fresh selected-guest sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuestAdmission {
    /// No supported guest probe is available, so new work stays unacquired.
    Unavailable,
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
            .finish()
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

/// Inputs for one production scoped admission attempt.
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
    pub(crate) release_read_secret: &'a ReleaseReadSecret,
    pub(crate) selected_engine: &'a SelectedEngineContext,
    pub(crate) worker_images: &'a VerifiedWorkerImages,
    pub(crate) guest_image: &'a GuestImageFile,
    pub(crate) resource_budget: ResourceBudget,
    pub(crate) allocated_header: u32,
    pub(crate) admission_state: &'a Path,
}

/// Acquire an offer using its exact journal-backed source authorization.
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
        resource_budget: Some(input.resource_budget),
        static_capacity: true,
        guest_admission: GuestAdmission::Unavailable,
        connection: Some(input.connection),
        authority_policy: Some(input.authority_policy),
        host_journal: Some(input.host_journal),
        release_read_secret: Some(input.release_read_secret),
        scoped_journal: Some(input.scoped_journal),
        allocated_header: input.allocated_header,
        source_authorized: true,
        admission_state: Some(input.admission_state),
        worker_images: Some(input.worker_images),
        selected_engine: Some(input.selected_engine),
        guest_image: Some(input.guest_image),
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
        input.release_read_secret,
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

/// Switch the client between the admin origin and the message host.
pub(crate) trait Lane {
    /// Use the admin origin for acquire, JIT, and session delete.
    ///
    /// # Errors
    ///
    /// Returns [`EnsureError`] when the origin cannot be selected.
    fn on_admin(&mut self) -> Result<(), EnsureError>;

    /// Use the message-host origin for acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns [`EnsureError`] when the origin cannot be selected.
    fn on_queue(&mut self) -> Result<(), EnsureError>;

    /// Use `https://api.github.com` for the runner directory.
    ///
    /// # Errors
    ///
    /// Returns [`EnsureError::Endpoint`] when the origin is not `https`.
    fn use_github_api(&mut self) -> Result<(), EnsureError>;
}
