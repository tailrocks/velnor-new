use super::super::ProbePhase;
use super::{
    RecoveryContext, RecoveryState, deadline_instant, quarantine_context,
    verify_fresh_recovery_engine,
};
use crate::{
    error::HostError,
    launch::resource_probe::publication::{VerifiedExecutionPublicationCleanup, execute},
};

pub(super) async fn resolve_container_id(
    context: &RecoveryContext<'_>,
    state: &RecoveryState,
) -> Result<String, HostError> {
    if let Some(id) = &context.row.container_id {
        return Ok(id.clone());
    }
    if context.row.phase != ProbePhase::CreateRequested {
        quarantine_context(context).await?;
        return Err(HostError::Ownership);
    }
    let response = execute::transport::inspect_optional(
        context.docker,
        &state.projection.name,
        deadline_instant(context.deadline),
    )
    .await?;
    let Some(response) = response else {
        quarantine_context(context).await?;
        return Err(HostError::Ownership);
    };
    let id = response.id.as_deref().ok_or(HostError::Ownership)?;
    if !recovery_identity_matches(state, &response, id) {
        quarantine_context(context).await?;
        return Err(HostError::Ownership);
    }
    context
        .deadline
        .within(
            context
                .journal
                .bind_execution_publication_container(&context.row.operation_id, id),
        )
        .await??;
    Ok(id.to_owned())
}

pub(super) async fn recover_container(
    context: &RecoveryContext<'_>,
    state: &RecoveryState,
    id: &str,
) -> Result<(), HostError> {
    let inspected = execute::transport::inspect_optional(
        context.docker,
        id,
        deadline_instant(context.deadline),
    )
    .await?;
    let Some(inspected) = inspected else {
        if context.row.phase == ProbePhase::RemoveRequested {
            return retire_removed(context, state, id).await;
        }
        quarantine_context(context).await?;
        return Err(HostError::Ownership);
    };
    if inspected.id.as_deref() != Some(id) || !recovery_identity_matches(state, &inspected, id) {
        quarantine_context(context).await?;
        return Err(HostError::Ownership);
    }
    let current = if context.row.container_id.is_some() {
        context.row.phase
    } else {
        ProbePhase::ContainerCreated
    };
    let current = stop_if_running(context, state, id, &inspected, current).await?;
    remove_stopped(context, state, id, current).await?;
    retire_removed(context, state, id).await
}

fn recovery_identity_matches(
    state: &RecoveryState,
    inspected: &bollard::models::ContainerInspectResponse,
    id: &str,
) -> bool {
    execute::inspect::matches_recovery_identity(
        &state.projection,
        &state.volume_name,
        &state.volume_mountpoint,
        inspected,
        id,
    ) && inspected.id.as_deref() == Some(id)
}

async fn stop_if_running(
    context: &RecoveryContext<'_>,
    state: &RecoveryState,
    id: &str,
    inspected: &bollard::models::ContainerInspectResponse,
    current: ProbePhase,
) -> Result<ProbePhase, HostError> {
    let status = inspected
        .state
        .as_ref()
        .and_then(|state| state.status)
        .ok_or(HostError::Ownership)?;
    if status != bollard::models::ContainerStateStatusEnum::RUNNING {
        if matches!(
            status,
            bollard::models::ContainerStateStatusEnum::CREATED
                | bollard::models::ContainerStateStatusEnum::EXITED
        ) {
            return Ok(current);
        }
        quarantine_context(context).await?;
        return Err(HostError::Ownership);
    }
    if !matches!(
        current,
        ProbePhase::Started
            | ProbePhase::StartRequested
            | ProbePhase::WaitRequested
            | ProbePhase::StopRequested
            | ProbePhase::RemoveRequested
    ) {
        quarantine_context(context).await?;
        return Err(HostError::Ownership);
    }
    let next = if matches!(
        current,
        ProbePhase::Started | ProbePhase::StartRequested | ProbePhase::WaitRequested
    ) {
        context
            .deadline
            .within(context.journal.transition_execution_publication(
                &context.row.operation_id,
                current,
                ProbePhase::StopRequested,
            ))
            .await??;
        ProbePhase::StopRequested
    } else {
        current
    };
    verify_fresh_recovery_engine(context.docker, context.row, context.deadline).await?;
    let stop = bollard::query_parameters::StopContainerOptionsBuilder::new()
        .t(1)
        .build();
    context
        .deadline
        .docker(context.docker.stop_container(id, Some(stop)))
        .await?
        .map_err(|_| HostError::Docker)?;
    let after_stop =
        execute::transport::inspect(context.docker, id, deadline_instant(context.deadline)).await?;
    if !recovery_identity_matches(state, &after_stop, id)
        || after_stop.state.as_ref().is_none_or(|state| {
            state.running != Some(false)
                || state.status != Some(bollard::models::ContainerStateStatusEnum::EXITED)
        })
    {
        return Err(HostError::Ownership);
    }
    Ok(next)
}

async fn remove_stopped(
    context: &RecoveryContext<'_>,
    state: &RecoveryState,
    id: &str,
    current: ProbePhase,
) -> Result<(), HostError> {
    if current != ProbePhase::RemoveRequested {
        context
            .deadline
            .within(context.journal.transition_execution_publication(
                &context.row.operation_id,
                current,
                ProbePhase::RemoveRequested,
            ))
            .await??;
    }
    let before =
        execute::transport::inspect(context.docker, id, deadline_instant(context.deadline)).await?;
    if !recovery_identity_matches(state, &before, id)
        || before.state.as_ref().is_none_or(|state| {
            state.running != Some(false)
                || !matches!(
                    state.status,
                    Some(
                        bollard::models::ContainerStateStatusEnum::CREATED
                            | bollard::models::ContainerStateStatusEnum::EXITED
                    )
                )
        })
    {
        return Err(HostError::Ownership);
    }
    verify_fresh_recovery_engine(context.docker, context.row, context.deadline).await?;
    context
        .deadline
        .docker(
            context.docker.remove_container(
                id,
                Some(
                    bollard::query_parameters::RemoveContainerOptionsBuilder::new()
                        .force(false)
                        .v(false)
                        .build(),
                ),
            ),
        )
        .await?
        .map_err(|_| HostError::Docker)?;
    if execute::transport::inspect_optional(context.docker, id, deadline_instant(context.deadline))
        .await?
        .is_some()
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

async fn retire_removed(
    context: &RecoveryContext<'_>,
    state: &RecoveryState,
    id: &str,
) -> Result<(), HostError> {
    verify_fresh_recovery_engine(context.docker, context.row, context.deadline).await?;
    let proof = VerifiedExecutionPublicationCleanup::after_exact_removal(
        &state.projection,
        id,
        ProbePhase::RemoveRequested,
    )?;
    context
        .deadline
        .within(context.journal.retire_execution_publication(proof))
        .await??;
    Ok(())
}
