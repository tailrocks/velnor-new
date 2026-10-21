use std::time::Duration;

use crate::{error::HostError, launch::scoped_runtime::ScopedChannelOutcome};

use super::recovery::{remember_acquired_recovery, report_acquired_recovery};
use super::{
    PositivePollContext, PreparedConnection, retain_authorization_pending,
    retain_bootstrap_pending, retain_ready_or_pending,
};

pub(super) async fn start_acquired(
    prepared: &mut PreparedConnection,
    acquired: crate::launch::steps::AcquiredProvisioning,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    let request_id = acquired.request().request_id();
    remember_acquired_recovery(prepared, request_id, &acquired);
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    let channel = match Box::pin(measure_acquired(prepared, acquired, context, deadline)).await {
        Ok(channel) => channel,
        Err(error) => {
            report_acquired_recovery(prepared, request_id, error);
            return Ok(());
        }
    };
    Box::pin(retain_scoped_channel(prepared, channel, context, deadline)).await
}

async fn measure_acquired(
    prepared: &mut PreparedConnection,
    acquired: crate::launch::steps::AcquiredProvisioning,
    context: &PositivePollContext<'_>,
    deadline: std::time::Instant,
) -> Result<ScopedChannelOutcome, HostError> {
    let Some(credentials) = prepared.credentials.as_mut() else {
        return Err(HostError::Keychain);
    };
    let admin = credentials.link.base().to_owned();
    let admin_token = credentials.link.token().to_owned();
    let mut lane = crate::launch::HostLane::new(&mut credentials.link, admin, None);
    let scoped_context = crate::launch::scoped_runtime::ScopedHostContext {
        docker: context.docker,
        host: context.journal,
        scoped: &prepared.scope,
        secret: context.release_read_secret,
        engine: context.selected_engine,
        images: context.worker_images,
    };
    #[cfg(test)]
    if let Some(provider) = context.start_permit_provider {
        return Box::pin(
            crate::launch::scoped_runtime::prepare_jit_and_measure_scoped_with_provider(
                crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired),
                &scoped_context,
                provider,
                &mut lane,
                &admin_token,
                deadline,
            ),
        )
        .await;
    }
    Box::pin(
        crate::launch::scoped_runtime::prepare_jit_and_measure_scoped(
            crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired),
            &scoped_context,
            &mut lane,
            &admin_token,
            deadline,
        ),
    )
    .await
}

async fn retain_scoped_channel(
    prepared: &mut PreparedConnection,
    channel: ScopedChannelOutcome,
    context: &PositivePollContext<'_>,
    deadline: std::time::Instant,
) -> Result<(), HostError> {
    let request_id = channel.request_id();
    match channel {
        ScopedChannelOutcome::NoStart => {}
        ScopedChannelOutcome::Ready(ready) => {
            let host_context = crate::launch::scoped_runtime::ScopedHostContext {
                docker: context.docker,
                host: context.journal,
                scoped: &prepared.scope,
                secret: context.release_read_secret,
                engine: context.selected_engine,
                images: context.worker_images,
            };
            let runner = Box::pin(crate::launch::scoped_runtime::prepare_listener(
                *ready,
                &host_context,
                &prepared.binding,
                &prepared.guest_image,
                deadline,
            ))
            .await;
            match runner {
                Ok(runner) => retain_ready_or_pending(prepared, runner),
                Err(pending) => retain_authorization_pending(prepared, request_id, pending),
            }
        }
        pending => retain_bootstrap_pending(prepared, request_id, pending),
    }
    Ok(())
}
