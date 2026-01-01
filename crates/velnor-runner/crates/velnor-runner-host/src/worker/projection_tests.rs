//! Docker projection checks. No Docker daemon or image is required.

use std::path::{Path, PathBuf};

use bollard::models::{HostConfigCgroupnsModeEnum, MountType};

use crate::error::HostError;
use crate::launch_identity::LaunchIdentity;

use super::{bollard_create, dind_create_for_identity, runner_create_for_identity};

const INSTANCE_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const INTENT_ID: i64 = 7;
const LAUNCH_ID: &str = "0123456789abcdef0123456789abcdef";
const ENGINE_ID: &str = "engine-test";
const ARCHIVE_TARGET: &str = "/opt/velnor/action-archives";
const ARCHIVE_ENV: &str = "ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/opt/velnor/action-archives";

fn identity() -> Result<LaunchIdentity, HostError> {
    LaunchIdentity::new(INSTANCE_ID, INTENT_ID, LAUNCH_ID, ENGINE_ID)
}

fn archive_cache_path() -> PathBuf {
    std::env::temp_dir().join(format!("velnor-action-archives-{}", std::process::id()))
}

#[test]
fn runner_and_dind_project_distinct_owned_names_and_labels() -> Result<(), HostError> {
    let identity = identity()?;
    let volume = identity.private_volume();
    assert_eq!(volume, format!("v{LAUNCH_ID}"));
    let runner = runner_create_for_identity(&identity, None)?;
    let dind = dind_create_for_identity(&identity)?;

    assert_eq!(
        runner.name,
        "velnor-runner-0123456789abcdef0123456789abcdef"
    );
    assert_eq!(dind.name, "velnor-dind-0123456789abcdef0123456789abcdef");
    assert_eq!(
        runner.labels,
        vec![
            "velnor.product=velnor".to_owned(),
            "velnor.instance=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            "velnor.launch=0123456789abcdef0123456789abcdef".to_owned(),
            "velnor.engine=engine-test".to_owned(),
            "velnor.role=runner".to_owned(),
            format!("velnor.volume={volume}"),
        ]
    );
    assert_eq!(
        dind.labels,
        vec![
            "velnor.product=velnor".to_owned(),
            "velnor.instance=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            "velnor.launch=0123456789abcdef0123456789abcdef".to_owned(),
            "velnor.engine=engine-test".to_owned(),
            "velnor.role=dind".to_owned(),
            format!("velnor.volume={volume}"),
        ]
    );

    let runner_create = bollard_create(&runner)?;
    let dind_create = bollard_create(&dind)?;
    for create in [&runner_create, &dind_create] {
        assert_eq!(
            create
                .config
                .host_config
                .as_ref()
                .and_then(|host| host.cgroupns_mode),
            Some(HostConfigCgroupnsModeEnum::PRIVATE)
        );
    }
    assert_eq!(
        runner_create.options.name.as_deref(),
        Some(runner.name.as_str())
    );
    assert_eq!(
        dind_create.options.name.as_deref(),
        Some(dind.name.as_str())
    );
    Ok(())
}

#[test]
fn runner_archive_cache_is_a_read_only_bind_in_docker_config() -> Result<(), HostError> {
    let identity = identity()?;
    let path = archive_cache_path();
    let source = path.to_str().ok_or(HostError::Path)?;
    let runner = runner_create_for_identity(&identity, Some(&path))?;

    assert_eq!(runner.env, vec![ARCHIVE_ENV]);
    assert!(
        runner
            .mounts
            .iter()
            .all(|mount| mount.target != ARCHIVE_TARGET)
    );
    assert_eq!(runner.bind_mounts.len(), 1);
    assert_eq!(runner.bind_mounts[0].source, source);
    assert_eq!(runner.bind_mounts[0].target, ARCHIVE_TARGET);
    assert!(runner.bind_mounts[0].read_only);

    let created = bollard_create(&runner)?;
    assert_eq!(created.config.env, Some(vec![ARCHIVE_ENV.to_owned()]));
    let host = created
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Docker)?;
    let mounts = host.mounts.as_ref().ok_or(HostError::Docker)?;
    let archive_mount = mounts
        .iter()
        .find(|mount| mount.target.as_deref() == Some(ARCHIVE_TARGET))
        .ok_or(HostError::Docker)?;
    assert_eq!(archive_mount.source.as_deref(), Some(source));
    assert_eq!(archive_mount.typ, Some(MountType::BIND));
    assert_eq!(archive_mount.read_only, Some(true));
    assert_eq!(
        archive_mount
            .bind_options
            .as_ref()
            .and_then(|options| options.create_mountpoint),
        Some(false)
    );
    Ok(())
}

#[test]
fn dind_gets_private_data_volume_without_runner_archive_projection() -> Result<(), HostError> {
    let identity = identity()?;
    let volume = identity.private_volume();
    let dind = dind_create_for_identity(&identity)?;

    assert_eq!(dind.env, [] as [std::string::String; 0]);
    assert_eq!(dind.cmd, [] as [std::string::String; 0]);
    assert_eq!(dind.bind_mounts, [] as [crate::worker::BindMount; 0]);
    assert!(
        dind.mounts
            .iter()
            .all(|mount| mount.target != ARCHIVE_TARGET)
    );
    assert!(dind.mounts.iter().any(|mount| {
        mount.source == format!("volume:{volume}-docker") && mount.target == "/var/lib/docker"
    }));

    let created = bollard_create(&dind)?;
    assert!(created.config.env.is_none());
    let host = created
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Docker)?;
    let mounts = host.mounts.as_ref().ok_or(HostError::Docker)?;
    let docker_data = mounts
        .iter()
        .find(|mount| mount.target.as_deref() == Some("/var/lib/docker"))
        .ok_or(HostError::Docker)?;
    let docker_data_source = format!("{volume}-docker");
    assert_eq!(
        docker_data.source.as_deref(),
        Some(docker_data_source.as_str())
    );
    assert_eq!(docker_data.typ, Some(MountType::VOLUME));
    assert!(
        mounts
            .iter()
            .all(|mount| mount.target.as_deref() != Some(ARCHIVE_TARGET))
    );
    Ok(())
}

#[test]
fn absent_or_relative_archive_path_does_not_create_a_bind() -> Result<(), HostError> {
    let identity = identity()?;
    let runner = runner_create_for_identity(&identity, None)?;
    assert_eq!(runner.env, [] as [std::string::String; 0]);
    assert_eq!(runner.bind_mounts, [] as [crate::worker::BindMount; 0]);

    assert_eq!(
        runner_create_for_identity(&identity, Some(Path::new("relative-cache"))).err(),
        Some(HostError::Path)
    );
    Ok(())
}
