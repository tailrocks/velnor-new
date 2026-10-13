//! `DinD` topology identity regressions.

use crate::error::HostError;
use crate::worker::dind_create_for_identity;

use super::{identity, inspect_projection, topology_matches};

#[test]
fn dind_topology_matches_its_image_command_and_closed_stdin() -> Result<(), HostError> {
    let spec = dind_create_for_identity(&identity()?)?;
    let inspected = inspect_projection(&spec)?;

    assert!(topology_matches(&spec, &inspected)?);
    Ok(())
}
