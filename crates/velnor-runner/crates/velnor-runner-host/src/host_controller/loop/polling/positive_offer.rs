use velnor_runner_github::Poll;

use crate::ResourceBudget;
use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::journal::ConnectionJournal;
use crate::launch::{
    PositiveAdmission, PositiveOfferAuthorization, ScopedAdmissionInput,
    acquire_authorized_scoped_offer, admit_when_ready, authorize_positive_offer,
};
use crate::listen::Link;
use crate::scale_set::EnsureError;
use crate::worker::guest_image::GuestImageFile;

use super::PositivePollContext;

pub(super) struct PositiveOfferInputs<'a, 'context> {
    pub(super) context: &'a PositivePollContext<'context>,
    pub(super) connection: &'a RepositoryConnection,
    pub(super) policy: &'a ApprovedJobAuthorityPolicy,
    pub(super) budget: ResourceBudget,
    pub(super) set_id: i64,
    pub(super) scoped: &'a ConnectionJournal,
    pub(super) guest_image: &'a GuestImageFile,
    pub(super) pat: &'a str,
    pub(super) allocated_header: u32,
}

pub(super) async fn admit_positive_offer(
    input: &PositiveOfferInputs<'_, '_>,
    link: &mut Link,
    polled: &Poll,
    queue_origin: Option<String>,
    queue_path: String,
    queue_token: String,
    admin_token: String,
) -> Result<PositiveAdmission, EnsureError> {
    let authorized = match authorize_positive_offer(
        input.connection,
        input.scoped,
        input.pat,
        input.set_id,
        polled,
    )
    .await?
    {
        PositiveOfferAuthorization::Deferred(authorized) => authorized,
        PositiveOfferAuthorization::TerminalNoAcquire(receipt) => {
            return Ok(PositiveAdmission::KnownNoAcquire(receipt));
        }
    };
    let admission = admit_when_ready(input.context.admission_ready, || async {
        let admission = acquire_authorized_scoped_offer(
            ScopedAdmissionInput {
                link,
                queue_origin,
                queue_path,
                queue_token,
                admin_token,
                set_id: input.set_id,
                polled,
                docker: input.context.docker,
                connection: input.connection,
                authority_policy: input.policy,
                pat: input.pat,
                host_journal: input.context.journal,
                scoped_journal: input.scoped,
                release_read_secret: input.context.release_read_secret,
                selected_engine: input.context.selected_engine,
                worker_images: input.context.worker_images,
                guest_image: input.guest_image,
                resource_budget: input.budget,
                allocated_header: input.allocated_header,
                admission_state: input.context.state,
            },
            &authorized,
        )
        .await?;
        Ok(admission_for_scoped_outcome(admission))
    })
    .await?;
    Ok(match admission {
        PositiveAdmission::NoAcquire => PositiveAdmission::Deferred(authorized),
        other => other,
    })
}

fn admission_for_scoped_outcome(
    outcome: crate::launch::steps::ScopedProvisioningOutcome,
) -> PositiveAdmission {
    match outcome {
        crate::launch::steps::ScopedProvisioningOutcome::NoStart => PositiveAdmission::NoAcquire,
        crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(receipt) => {
            PositiveAdmission::KnownNoAcquire(receipt)
        }
        crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired) => {
            PositiveAdmission::Acquired(acquired)
        }
        crate::launch::steps::ScopedProvisioningOutcome::Recovered(recovery) => {
            PositiveAdmission::Acquired(recovery.into_acquired())
        }
    }
}
