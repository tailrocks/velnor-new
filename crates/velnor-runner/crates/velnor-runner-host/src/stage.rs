//! Identity-checked Docker cleanup for recorded worker containers.

use bollard::Docker;

use crate::docker_spec::{DeleteDecision, delete_decision};
use crate::error::HostError;

mod engine;
pub(crate) use engine::PairEngine;

/// Inspect `name`, then delete `owned_id` only when it still names that id.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the name is empty, its identity cannot be
/// established, or the owned container cannot be removed.
pub async fn remove_recorded(
    docker: &Docker,
    owned_id: &str,
    name: &str,
) -> Result<DeleteDecision, HostError> {
    decide(docker, owned_id, name).await
}

pub(crate) async fn decide<E: PairEngine + ?Sized>(
    engine: &E,
    owned_id: &str,
    name: &str,
) -> Result<DeleteDecision, HostError> {
    let observed = engine.id_for_name(name).await?;
    let decision = delete_decision(owned_id, observed.as_deref());
    if decision == DeleteDecision::Delete {
        engine.remove(owned_id).await?;
    }
    Ok(decision)
}

#[cfg(test)]
mod tests;
