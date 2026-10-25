use crate::error::HostError;
use crate::worker::{dind_create_for_identity, runner_create_for_identity};

use super::{identity, inspect_projection, topology_matches};

#[test]
fn runner_topology_rejects_tty_drift() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;
    let mut inspected = inspect_projection(&spec)?;
    inspected.config.as_mut().ok_or(HostError::Ownership)?.tty = Some(true);

    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn dind_topology_rejects_entrypoint_override() -> Result<(), HostError> {
    let spec = dind_create_for_identity(&identity()?)?;
    let mut inspected = inspect_projection(&spec)?;
    inspected
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .entrypoint = Some(vec!["/bin/echo".to_owned()]);

    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}
