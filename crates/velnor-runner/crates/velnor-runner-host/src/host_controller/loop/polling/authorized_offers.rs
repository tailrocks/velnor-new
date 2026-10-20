//! Resume an already-authorized offer before polling for new work.

use crate::{
    config::{ApprovedJobAuthorityPolicy, RepositoryConnection},
    error::HostError,
    journal::HeaderAllocation,
    launch::{Drive, GuestAdmission, Rest},
};

use super::{PositivePollContext, acquired::start_acquired};
use crate::host_controller::loop_driver::state::PreparedConnection;

pub(super) async fn resume(
    prepared: &mut PreparedConnection,
    allocation: &HeaderAllocation,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    if !context.admission_ready || allocation.target() == 0 {
        return Ok(());
    }
    let offers = prepared.scope.authorized_offers(16).await?;
    let Some(offer) = offers.into_iter().next() else {
        return Ok(());
    };
    if offer.connection_id() != &prepared.binding.connection_id
        || offer.scale_set_id() != prepared.set_id
    {
        return Err(HostError::Identity);
    }
    validate_source_binding(prepared, &offer).await?;
    let connection = context
        .config
        .connection(&prepared.binding.connection_id)
        .ok_or(HostError::Config)?;
    let policy = context
        .config
        .validated_job_authority_policy(&prepared.binding.connection_id)?
        .ok_or(HostError::Config)?;
    resume_offer(
        prepared,
        allocation,
        context,
        offer.request_id(),
        connection,
        policy,
    )
    .await
}

async fn validate_source_binding(
    prepared: &PreparedConnection,
    offer: &crate::journal::AuthorizedOffer,
) -> Result<(), HostError> {
    let source_binding = prepared.scope.source_authorization_binding().await?;
    if offer.binding_digest() != source_binding.binding_digest()
        || source_binding.scale_set_id() != prepared.set_id
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn resume_offer(
    prepared: &mut PreparedConnection,
    allocation: &HeaderAllocation,
    context: &PositivePollContext<'_>,
    request_id: i64,
    connection: &RepositoryConnection,
    policy: &ApprovedJobAuthorityPolicy,
) -> Result<(), HostError> {
    let Some(credentials) = prepared.credentials.as_mut() else {
        return Err(HostError::Keychain);
    };
    let Some(permit) = crate::launch::start_permit(
        context.docker,
        context.journal,
        context.config.resource_budget()?,
        context.release_read_secret,
    )
    .await
    else {
        return Ok(());
    };
    let (owner, repo) = connection
        .repository
        .split_once('/')
        .ok_or(HostError::Config)?;
    let mut drive = Drive::from_rest(
        prepared.set_id,
        credentials.session.message_queue_url.clone(),
        credentials.session.token().to_owned(),
        credentials.link.token().to_owned(),
        Rest {
            owner,
            repo,
            pat: credentials.github_pat.expose(),
            resource_budget: Some(context.config.resource_budget()?),
            static_capacity: true,
            guest_admission: GuestAdmission::Unavailable,
            connection: Some(connection),
            authority_policy: Some(policy),
            host_journal: Some(context.journal),
            release_read_secret: Some(context.release_read_secret),
            scoped_journal: Some(&prepared.scope),
            allocated_header: allocation.target(),
            source_authorized: true,
            admission_state: Some(context.state),
            worker_images: Some(context.worker_images),
            selected_engine: Some(context.selected_engine),
            guest_image: Some(&prepared.guest_image),
        },
    );
    drive.docker_engine_id = Some(context.selected_engine.engine_id().to_owned());
    let outcome = acquire_offer(context, &mut credentials.link, &drive, request_id, permit).await?;
    if let crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired) = outcome {
        start_acquired(prepared, acquired, context).await?;
    }
    Ok(())
}

async fn acquire_offer(
    context: &PositivePollContext<'_>,
    link: &mut crate::listen::Link,
    drive: &Drive,
    request_id: i64,
    permit: crate::launch::StartPermit,
) -> Result<crate::launch::steps::ScopedProvisioningOutcome, HostError> {
    let admin = link.base().to_owned();
    let mut lane = crate::launch::HostLane::new(link, admin, None);
    let outcome = crate::launch::steps::acquire_authorized_request(
        &mut lane,
        drive,
        request_id,
        permit,
        context.selected_engine,
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(outcome)
}
