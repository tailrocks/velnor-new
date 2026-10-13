use bollard::models::HostConfigCgroupnsModeEnum;

use crate::error::HostError;
use crate::worker::{dind_create_for_identity, runner_create_for_identity};

use super::super::topology_matches;
use super::{identity, inspect_projection};

#[test]
fn runner_and_dind_reconciliation_require_private_cgroup_namespaces() -> Result<(), HostError> {
    for spec in [
        runner_create_for_identity(&identity()?, None)?,
        dind_create_for_identity(&identity()?)?,
    ] {
        let valid = inspect_projection(&spec)?;
        assert!(topology_matches(&spec, &valid)?);
        for mode in [Some(HostConfigCgroupnsModeEnum::HOST), None] {
            let mut inspected = valid.clone();
            inspected
                .host_config
                .as_mut()
                .ok_or(HostError::Ownership)?
                .cgroupns_mode = mode;
            assert!(!topology_matches(&spec, &inspected)?);
        }
    }
    Ok(())
}
