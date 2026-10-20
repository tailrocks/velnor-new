use super::{
    Docker, IntakeCompositionProvider, Journal, RecoveryObservation, ResourceBudget,
    SelectedEngineContext, StartPermit, lifecycle,
};

pub(crate) async fn start_permit_with_test_provider(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    provider: &IntakeCompositionProvider,
) -> Option<StartPermit> {
    match Box::pin(lifecycle::collect(docker, journal, provider)).await {
        Ok(Some(sample)) => sample.into_start_permit(budget),
        Ok(None) | Err(_) => None,
    }
}

pub(crate) async fn recovery_observation_with_provider(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    charge: (u64, u64),
    engine: &SelectedEngineContext,
    provider: &IntakeCompositionProvider,
) -> Option<RecoveryObservation> {
    let sample = Box::pin(lifecycle::collect(docker, journal, provider))
        .await
        .ok()??;
    sample.into_recovery_observation(
        budget,
        charge,
        engine.engine_id(),
        engine.docker_root_digest(),
    )
}
