use bollard::models::{Mount, MountType, MountVolumeOptions};

use crate::error::HostError;
use crate::worker::{dind_create_for_identity, runner_create_for_identity};
use velnor_runner_core::runner_work_path;

use super::{identity, inspect_projection, topology_matches};

#[test]
fn topology_accepts_no_copy_for_home_and_work_roles() -> Result<(), HostError> {
    for spec in [
        runner_create_for_identity(&identity()?, None)?,
        dind_create_for_identity(&identity()?)?,
    ] {
        let inspected = inspect_projection(&spec)?;
        assert!(topology_matches(&spec, &inspected)?);
    }
    Ok(())
}

#[test]
fn topology_rejects_missing_false_duplicate_and_changed_private_mounts() -> Result<(), HostError> {
    let work_target = runner_work_path();
    for spec in [
        runner_create_for_identity(&identity()?, None)?,
        dind_create_for_identity(&identity()?)?,
    ] {
        let valid = inspect_projection(&spec)?;
        for target in ["/home/runner", work_target.as_str()] {
            let mut missing_options = valid.clone();
            mount_mut(&mut missing_options, target)?.volume_options = None;
            assert!(!topology_matches(&spec, &missing_options)?);

            let mut no_copy_false = valid.clone();
            mount_mut(&mut no_copy_false, target)?.volume_options = Some(MountVolumeOptions {
                no_copy: Some(false),
                ..Default::default()
            });
            assert!(!topology_matches(&spec, &no_copy_false)?);

            let mut unexpected_options = valid.clone();
            mount_mut(&mut unexpected_options, target)?.volume_options = Some(MountVolumeOptions {
                no_copy: Some(true),
                labels: Some(HashMap::default()),
                ..Default::default()
            });
            assert!(!topology_matches(&spec, &unexpected_options)?);

            let mut changed_source = valid.clone();
            mount_mut(&mut changed_source, target)?.source = Some("foreign-volume".to_owned());
            assert!(!topology_matches(&spec, &changed_source)?);

            let mut changed_type = valid.clone();
            mount_mut(&mut changed_type, target)?.typ = Some(MountType::BIND);
            assert!(!topology_matches(&spec, &changed_type)?);

            let mut duplicate = valid.clone();
            let duplicate_mount = mount_mut(&mut duplicate, target)?.clone();
            duplicate
                .host_config
                .as_mut()
                .ok_or(HostError::Ownership)?
                .mounts
                .as_mut()
                .ok_or(HostError::Ownership)?
                .push(duplicate_mount);
            assert!(!topology_matches(&spec, &duplicate)?);
        }

        let mut missing_mounts = valid;
        missing_mounts
            .host_config
            .as_mut()
            .ok_or(HostError::Ownership)?
            .mounts = None;
        assert!(!topology_matches(&spec, &missing_mounts)?);
    }
    Ok(())
}

fn mount_mut<'a>(
    inspected: &'a mut bollard::models::ContainerInspectResponse,
    target: &str,
) -> Result<&'a mut Mount, HostError> {
    inspected
        .host_config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .mounts
        .as_mut()
        .ok_or(HostError::Ownership)?
        .iter_mut()
        .find(|mount| mount.target.as_deref() == Some(target))
        .ok_or(HostError::Ownership)
}
use std::collections::HashMap;
