//! Docker inspect label tests, including image-inherited OCI labels.

use std::collections::HashMap;

use bollard::models::{
    ContainerConfig, ContainerInspectResponse, HostConfig, HostConfigCgroupnsModeEnum, MountPoint,
};

use crate::error::HostError;
use crate::launch_identity::LaunchIdentity;
use crate::worker::mounts::label_map;
use crate::worker::{CreateProjection, dind_create_for_identity, runner_create_for_identity};

use super::{inspect_labels_match, same_launch, topology_matches, validate_existing_row};

#[path = "containers_cgroupns_tests.rs"]
mod cgroupns;
#[path = "containers_environment_tests.rs"]
mod environment_tests;
#[path = "containers_limits_tests.rs"]
mod limits;
#[path = "containers_security_tests.rs"]
mod security;
#[path = "containers_topology_tests.rs"]
mod topology;

const UBUNTU_PATH: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

fn identity() -> Result<LaunchIdentity, HostError> {
    LaunchIdentity::new(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        7,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "engine-test",
    )
}

fn inspect_with_labels(labels: HashMap<String, String>) -> ContainerInspectResponse {
    ContainerInspectResponse {
        config: Some(ContainerConfig {
            labels: Some(labels),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn inspect_projection(spec: &CreateProjection) -> Result<ContainerInspectResponse, HostError> {
    let labels = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    let budget = spec.resource_budget;
    let limits = if spec.privileged {
        budget.dind()
    } else {
        budget.runner()
    };
    let mut mounts = Vec::with_capacity(spec.mounts.len() + spec.bind_mounts.len());
    let mut env = vec![UBUNTU_PATH.to_owned()];
    env.extend(spec.env.iter().cloned());
    for mount in &spec.mounts {
        let name = mount
            .source
            .strip_prefix("volume:")
            .ok_or(HostError::Ownership)?;
        mounts.push(MountPoint {
            typ: Some("volume".to_owned()),
            name: Some(name.to_owned()),
            destination: Some(mount.target.clone()),
            rw: Some(true),
            ..Default::default()
        });
    }
    for mount in &spec.bind_mounts {
        mounts.push(MountPoint {
            typ: Some("bind".to_owned()),
            source: Some(mount.source.clone()),
            destination: Some(mount.target.clone()),
            rw: Some(!mount.read_only),
            ..Default::default()
        });
    }
    Ok(ContainerInspectResponse {
        config: Some(ContainerConfig {
            image: Some(spec.image.clone()),
            labels: Some(labels),
            env: Some(env),
            cmd: (!spec.cmd.is_empty()).then(|| spec.cmd.clone()),
            entrypoint: Some(spec.entrypoint.clone()),
            user: spec.user.clone(),
            working_dir: spec.working_dir.clone(),
            attach_stdin: Some(spec.open_stdin),
            open_stdin: Some(spec.open_stdin),
            stdin_once: Some(spec.open_stdin),
            tty: Some(false),
            ..Default::default()
        }),
        host_config: Some(HostConfig {
            cgroupns_mode: Some(HostConfigCgroupnsModeEnum::PRIVATE),
            privileged: Some(spec.privileged),
            readonly_rootfs: Some(!spec.privileged),
            cap_drop: (!spec.privileged).then(|| vec!["ALL".to_owned()]),
            cap_add: (!spec.privileged)
                .then(|| vec!["SETGID".to_owned(), "SETUID".to_owned()]),
            security_opt: (!spec.privileged)
                .then(|| vec![super::super::super::seccomp::runner_security_opt()]),
            network_mode: spec.network_mode.clone(),
            nano_cpus: Some(limits.nano_cpus),
            memory: Some(limits.memory_bytes),
            memory_swap: Some(limits.memory_bytes),
            ..Default::default()
        }),
        mounts: Some(mounts),
        ..Default::default()
    })
}

#[test]
fn inspect_accepts_inherited_oci_labels_with_exact_velnor_identity() -> Result<(), HostError> {
    let spec = dind_create_for_identity(&identity()?)?;
    let expected = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    let mut actual = expected.clone();
    actual.insert(
        "org.opencontainers.image.version".to_owned(),
        "26.04".to_owned(),
    );
    let inspected = inspect_with_labels(actual.clone());

    assert!(inspect_labels_match(&expected, &inspected));
    assert_eq!(validate_existing_row(&expected, &actual)?, "dind");
    Ok(())
}

#[test]
fn runner_preflight_accepts_its_existing_dind_role() -> Result<(), HostError> {
    let identity = identity()?;
    let runner = runner_create_for_identity(&identity, None)?;
    let dind = dind_create_for_identity(&identity)?;
    let expected = label_map(&runner.labels)?.ok_or(HostError::Ownership)?;
    let mut actual = label_map(&dind.labels)?.ok_or(HostError::Ownership)?;
    actual.insert(
        "org.opencontainers.image.version".to_owned(),
        "26.04".to_owned(),
    );

    assert_eq!(
        expected.get("velnor.role").map(String::as_str),
        Some("runner")
    );
    assert_eq!(actual.get("velnor.role").map(String::as_str), Some("dind"));
    assert!(same_launch(&expected, &actual));
    assert_eq!(validate_existing_row(&expected, &actual)?, "dind");
    Ok(())
}

#[test]
fn inspect_rejects_mismatched_velnor_identity_values() -> Result<(), HostError> {
    let spec = dind_create_for_identity(&identity()?)?;
    let expected = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    let mut actual = expected.clone();
    actual.insert(
        "velnor.launch".to_owned(),
        "cccccccccccccccccccccccccccccccc".to_owned(),
    );

    assert!(!inspect_labels_match(
        &expected,
        &inspect_with_labels(actual.clone())
    ));
    assert!(!same_launch(&expected, &actual));
    assert_eq!(
        validate_existing_row(&expected, &actual),
        Err(HostError::Ownership)
    );
    Ok(())
}

#[test]
fn inspect_rejects_mislabeled_container_role() -> Result<(), HostError> {
    let spec = dind_create_for_identity(&identity()?)?;
    let expected = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    let mut actual = expected.clone();
    actual.insert("velnor.role".to_owned(), "runner".to_owned());

    assert!(!inspect_labels_match(
        &expected,
        &inspect_with_labels(actual.clone())
    ));
    assert_eq!(validate_existing_row(&expected, &actual), Ok("runner"));
    Ok(())
}

#[test]
fn inspect_rejects_foreign_velnor_labels() -> Result<(), HostError> {
    let spec = dind_create_for_identity(&identity()?)?;
    let expected = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    let mut actual = expected.clone();
    actual.insert("velnor.owner".to_owned(), "foreign".to_owned());

    assert!(!inspect_labels_match(
        &expected,
        &inspect_with_labels(actual.clone())
    ));
    assert!(!same_launch(&expected, &actual));
    assert_eq!(
        validate_existing_row(&expected, &actual),
        Err(HostError::Ownership)
    );
    Ok(())
}

#[test]
fn runner_topology_matches_inspected_projection() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;
    let inspected = inspect_projection(&spec)?;

    assert!(topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn runner_topology_rejects_command_drift() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;
    let mut inspected = inspect_projection(&spec)?;
    inspected.config.as_mut().ok_or(HostError::Ownership)?.cmd = Some(vec!["/bin/sh".to_owned()]);

    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn runner_topology_rejects_entrypoint_override() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;
    let mut inspected = inspect_projection(&spec)?;
    inspected
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .entrypoint = Some(vec!["/bin/echo".to_owned()]);

    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn runner_topology_rejects_stdin_flag_drift() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;
    let valid = inspect_projection(&spec)?;

    let mut attach_closed = valid.clone();
    attach_closed
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .attach_stdin = Some(false);
    assert!(!topology_matches(&spec, &attach_closed)?);

    let mut stdin_closed = valid.clone();
    stdin_closed
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .open_stdin = Some(false);
    assert!(!topology_matches(&spec, &stdin_closed)?);

    let mut stdin_once_disabled = valid;
    stdin_once_disabled
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .stdin_once = Some(false);
    assert!(!topology_matches(&spec, &stdin_once_disabled)?);
    Ok(())
}

#[test]
fn runner_topology_rejects_tty_drift() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;
    let mut inspected = inspect_projection(&spec)?;
    inspected.config.as_mut().ok_or(HostError::Ownership)?.tty = Some(true);

    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn runner_topology_rejects_user_and_working_directory_drift() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;
    let valid = inspect_projection(&spec)?;

    let mut wrong_user = valid.clone();
    wrong_user.config.as_mut().ok_or(HostError::Ownership)?.user = Some("root".to_owned());
    assert!(!topology_matches(&spec, &wrong_user)?);

    let mut wrong_directory = valid;
    wrong_directory
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .working_dir = Some("/".to_owned());
    assert!(!topology_matches(&spec, &wrong_directory)?);
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
