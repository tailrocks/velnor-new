use bollard::models::MountType;

use super::{bollard, projection};
use crate::{HostError, bollard_create, dind_create, runner_create, runner_plan};

#[test]
fn runner_create_opens_stdin_and_is_not_privileged() -> Result<(), HostError> {
    let spec = projection("worker_a")?;
    assert_eq!(spec.name, "worker_a-runner");
    assert!(spec.open_stdin);
    assert!(!spec.privileged);
    assert_eq!(spec.platform, "linux/amd64");
    assert_eq!(spec.image, "velnor-runner:ubuntu-26.04-2.337.0");
    assert_eq!(spec.mounts.len(), 2);
    assert_eq!(spec.mounts[0].source, "volume:worker_a");
    assert_eq!(spec.mounts[0].target, "/run");
    assert!(
        spec.mounts
            .iter()
            .all(|mount| mount.target != "/var/lib/docker")
    );
    assert_eq!(spec.env, Vec::<String>::new());
    assert!(spec.network_mode.is_none());
    Ok(())
}

#[test]
fn runner_create_rejects_jit_in_env_or_cmd() -> Result<(), HostError> {
    let mut plan = runner_plan("worker_a")?;
    plan.env
        .push("ACTIONS_RUNNER_INPUT_JITCONFIG=canary".to_owned());
    assert_eq!(runner_create(&plan), Err(HostError::ForbiddenMount));

    let mut plan = runner_plan("worker_a")?;
    plan.cmd.push("canary-jit".to_owned());
    assert_eq!(runner_create(&plan), Err(HostError::ForbiddenMount));
    Ok(())
}

#[test]
fn dind_is_privileged_and_shares_the_runner_volumes() -> Result<(), HostError> {
    for name in ["", "a/b", "a b", ".hidden", "has:colon"] {
        assert_eq!(dind_create(name).err(), runner_plan(name).err(), "{name}");
    }
    let spec = dind_create("worker_a")?;
    assert_eq!(spec.name, "worker_a-dind");
    assert!(spec.labels.contains(&"velnor.worker=worker_a".to_owned()));
    assert!(spec.labels.contains(&"velnor.role=dind".to_owned()));
    assert!(spec.privileged);
    assert!(!spec.open_stdin);
    assert_eq!(spec.env, Vec::<String>::new());
    assert_eq!(spec.cmd, Vec::<String>::new());
    assert_eq!(spec.image, "velnor-dind:29.8.2");
    assert_eq!(spec.platform, "linux/amd64");
    let runner = runner_plan("worker_a")?;
    assert_eq!(&spec.mounts[..runner.mounts.len()], &runner.mounts[..]);
    assert!(
        !runner
            .mounts
            .iter()
            .any(|mount| mount.target == "/var/lib/docker")
    );
    assert_eq!(
        spec.mounts
            .last()
            .map(|mount| (mount.source.as_str(), mount.target.as_str())),
        Some(("volume:worker_a-docker", "/var/lib/docker"))
    );
    assert!(spec.network_mode.is_none());
    Ok(())
}

#[test]
fn runner_joins_only_its_dind_netns() -> Result<(), HostError> {
    let spec = projection("worker_a")?;
    let not_hex = "g".repeat(64);
    for bad in [
        "",
        "host",
        "container:abc",
        "../id",
        "short",
        not_hex.as_str(),
    ] {
        assert_eq!(
            crate::worker::join_dind_net(spec.clone(), bad).err(),
            Some(HostError::ForbiddenMount),
            "{bad}"
        );
    }
    let id = "a".repeat(64);
    let mode = format!("container:{id}");
    let joined = crate::worker::join_dind_net(spec, &id)?;
    assert!(!joined.privileged);
    assert_eq!(joined.network_mode.as_deref(), Some(mode.as_str()));
    let created = bollard_create(&joined)?;
    let host = created
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Docker)?;
    assert_eq!(host.privileged, Some(false));
    assert_eq!(host.network_mode.as_deref(), Some(mode.as_str()));
    let dind = bollard_create(&dind_create("worker_a")?)?;
    let dind_host = dind.config.host_config.as_ref().ok_or(HostError::Docker)?;
    assert!(dind_host.network_mode.is_none());
    Ok(())
}

#[test]
fn bollard_config_from_a_clean_plan_omits_canary() -> Result<(), HostError> {
    let created = bollard("worker_a")?;
    assert_eq!(created.options.name.as_deref(), Some("worker_a-runner"));
    assert_eq!(created.options.platform, "linux/amd64");
    assert_eq!(created.config.open_stdin, Some(true));
    let host = created
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Docker)?;
    assert_eq!(host.privileged, Some(false));
    let text = format!("{created:?}");
    assert!(!text.contains("canary-jit"));
    assert!(!text.to_ascii_lowercase().contains("jitconfig"));

    let dind = bollard_create(&dind_create("worker_a")?)?;
    assert_eq!(dind.options.name.as_deref(), Some("worker_a-dind"));
    assert_eq!(dind.options.platform, "linux/amd64");
    assert_eq!(dind.config.open_stdin, Some(false));
    assert!(dind.config.env.is_none());
    assert!(dind.config.cmd.is_none());
    let host = dind.config.host_config.as_ref().ok_or(HostError::Docker)?;
    assert_eq!(host.privileged, Some(true));
    let mounts = host.mounts.as_ref().ok_or(HostError::Docker)?;
    assert_eq!(mounts.len(), 3);
    assert_eq!(mounts[0].target.as_deref(), Some("/run"));
    assert_eq!(mounts[0].source.as_deref(), Some("worker_a"));
    assert_eq!(mounts[0].typ, Some(MountType::VOLUME));
    assert_eq!(mounts[1].target.as_deref(), Some("/home/runner/_work"));
    assert_eq!(mounts[1].source.as_deref(), Some("worker_a-work"));
    assert_eq!(mounts[1].typ, Some(MountType::VOLUME));
    assert_eq!(mounts[2].target.as_deref(), Some("/var/lib/docker"));
    assert_eq!(mounts[2].source.as_deref(), Some("worker_a-docker"));
    assert_eq!(mounts[2].typ, Some(MountType::VOLUME));
    let text = format!("{dind:?}");
    assert!(!text.contains("canary-jit"));
    assert!(!text.contains("/var/run/docker.sock"));
    Ok(())
}

#[test]
fn bollard_create_rejects_a_host_bind() -> Result<(), HostError> {
    let mut spec = dind_create("worker_a")?;
    spec.mounts[0].source = "/var/run/docker.sock".to_owned();
    assert_eq!(bollard_create(&spec), Err(HostError::ForbiddenMount));
    Ok(())
}

#[test]
fn malformed_ownership_label_is_rejected() -> Result<(), HostError> {
    let mut spec = projection("worker_a")?;
    spec.labels.push("velnor.volume".to_owned());
    assert_eq!(bollard_create(&spec), Err(HostError::ForbiddenMount));
    Ok(())
}
