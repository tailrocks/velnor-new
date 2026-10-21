use super::*;

pub(crate) async fn prepare_jit_and_measure_scoped_with_provider<T>(
    outcome: ScopedProvisioningOutcome,
    docker: &Docker,
    scoped: &ConnectionJournal,
    host: &Journal,
    provider: &crate::launch::IntakeCompositionProvider,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    lane: &mut T,
    admin_token: &str,
    deadline: Instant,
) -> Result<ScopedChannelOutcome, HostError>
where
    T: Transport + Lane,
{
    let bound = match provider_entry::prepare_and_bind_scoped_with_test_provider(
        outcome, docker, scoped, host, provider, engine, images,
    )
    .await?
    {
        ScopedRuntimeOutcome::NoStart => return Ok(ScopedChannelOutcome::NoStart),
        ScopedRuntimeOutcome::Pending(pending) => {
            return Ok(ScopedChannelOutcome::PendingPreparation(pending));
        }
        ScopedRuntimeOutcome::Bound(bound) => *bound,
    };
    let jit = match create_and_bind(bound, lane, admin_token, scoped).await {
        Ok(jit) => jit,
        Err(pending) => return Ok(ScopedChannelOutcome::PendingJit(pending)),
    };
    match jit.measure_channel(docker, engine, images, deadline).await {
        Ok(ready) => Ok(ScopedChannelOutcome::Ready(Box::new(ready))),
        Err(pending) => Ok(ScopedChannelOutcome::PendingChannel(pending)),
    }
}
