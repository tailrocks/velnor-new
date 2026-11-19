// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::Docker;
use bollard::models::ContainerInspectResponse;
use bollard::query_parameters::ListVolumesOptions;
use velnor_runner_host::HostError;
use velnor_runner_host::local::{
    ENVIRONMENT_LABEL, LocalPairRequest, Removed, StartFailure, remove_pair, start_pair,
};

#[path = "local_pair_public/failure.rs"]
mod failure;
#[expect(
    dead_code,
    reason = "Each test binary uses part of the shared pair fixtures."
)]
#[path = "local_pair_public/support.rs"]
mod support;
use support::{Fixture, TestResult, labelled, request};

const TMPFS: [(&str, &str); 2] = [
    (
        "/tmp",
        "rw,exec,nosuid,nodev,size=2147483648,nr_inodes=131072",
    ),
    (
        "/dev/shm",
        "rw,exec,nosuid,nodev,size=268435456,nr_inodes=16384",
    ),
];

struct Observed {
    dind: ContainerInspectResponse,
    runner: ContainerInspectResponse,
    dind_id: String,
    label_keys: Vec<String>,
    removed: Removed,
    left: (usize, usize),
}

async fn volume_label_keys(docker: &Docker, environment: &str) -> TestResult<Vec<String>> {
    let filters = HashMap::from([(
        "label".to_owned(),
        vec![format!("{ENVIRONMENT_LABEL}={environment}")],
    )]);
    let volumes = docker
        .list_volumes(Some(ListVolumesOptions {
            filters: Some(filters),
        }))
        .await?;
    Ok(volumes
        .volumes
        .unwrap_or_default()
        .into_iter()
        .flat_map(|volume| volume.labels.into_keys())
        .collect())
}

fn container_label_keys(container: &ContainerInspectResponse) -> Vec<String> {
    container
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .map(|labels| labels.keys().cloned().collect())
        .unwrap_or_default()
}

async fn observe_pair(fixture: &Fixture) -> TestResult<Observed> {
    let docker = &fixture.docker;
    let pair = Box::pin(start_pair(docker, &fixture.request())).await?;
    let dind = docker.inspect_container(pair.dind_id(), None).await;
    let runner = docker.inspect_container(pair.runner_id(), None).await;
    let volume_keys = volume_label_keys(docker, &fixture.environment).await;
    let removed = remove_pair(docker, &pair).await?;
    let (dind, runner) = (dind?, runner?);
    let mut label_keys = volume_keys?;
    label_keys.extend(container_label_keys(&dind));
    label_keys.extend(container_label_keys(&runner));
    Ok(Observed {
        dind,
        runner,
        dind_id: pair.dind_id().to_owned(),
        label_keys,
        removed,
        left: labelled(docker, &fixture.environment).await?,
    })
}

fn tmpfs(container: &ContainerInspectResponse) -> Option<HashMap<String, String>> {
    container.host_config.as_ref()?.tmpfs.clone()
}

fn limits(container: &ContainerInspectResponse) -> Option<(i64, i64, i64, i64, bool)> {
    let host = container.host_config.as_ref()?;
    Some((
        host.nano_cpus?,
        host.memory?,
        host.memory_swap?,
        host.pids_limit?,
        host.privileged?,
    ))
}

fn networks(container: &ContainerInspectResponse) -> Vec<String> {
    container
        .network_settings
        .as_ref()
        .and_then(|settings| settings.networks.as_ref())
        .map(|networks| networks.keys().cloned().collect())
        .unwrap_or_default()
}

fn network_mode(container: &ContainerInspectResponse) -> Option<String> {
    container.host_config.as_ref()?.network_mode.clone()
}

fn running(container: &ContainerInspectResponse) -> bool {
    container
        .state
        .as_ref()
        .and_then(|state| state.running)
        .unwrap_or(false)
}

#[tokio::test]
async fn local_pair_starts_on_the_named_network_and_is_removed_by_its_label() -> TestResult<()> {
    let Some(fixture) = Fixture::new().await? else {
        return Ok(());
    };
    let network = fixture.network.clone();
    let observed = Box::pin(observe_pair(&fixture)).await;
    fixture.finish().await?;
    let observed = observed?;
    let expected_tmpfs = TMPFS
        .iter()
        .map(|(target, options)| ((*target).to_owned(), (*options).to_owned()))
        .collect::<HashMap<_, _>>();

    assert!(running(&observed.dind));
    assert!(running(&observed.runner));
    assert_eq!(networks(&observed.dind), vec![network.clone()]);
    assert_eq!(network_mode(&observed.dind), Some(network));
    assert_eq!(
        network_mode(&observed.runner),
        Some(format!("container:{}", observed.dind_id))
    );
    let dind_memory = 40_063_991_808;
    let runner_memory = 22_884_122_624;
    assert_eq!(
        limits(&observed.dind),
        Some((500_000_000, dind_memory, dind_memory, 2048, true))
    );
    assert_eq!(
        limits(&observed.runner),
        Some((1_500_000_000, runner_memory, runner_memory, 1024, false))
    );
    assert_eq!(tmpfs(&observed.dind), Some(expected_tmpfs.clone()));
    assert_eq!(tmpfs(&observed.runner), Some(expected_tmpfs));
    assert_eq!(
        observed.removed,
        Removed {
            containers: 2,
            volumes: 5
        }
    );
    assert_eq!(observed.left, (0, 0));
    assert!(
        observed
            .label_keys
            .iter()
            .any(|key| key == ENVIRONMENT_LABEL)
    );
    assert_eq!(
        observed
            .label_keys
            .iter()
            .filter(|key| key.starts_with("io.velnor."))
            .count(),
        0
    );
    Ok(())
}

#[tokio::test]
async fn local_pair_refuses_the_host_network_before_creating_anything() -> TestResult<()> {
    let Some(docker) = support::docker()? else {
        return Ok(());
    };
    let environment = format!("test-{}", support::suffix()?);
    let refused = Box::pin(start_pair(
        &docker,
        &request(&environment, "host", &format!("sha256:{}", "a".repeat(64))),
    ))
    .await;
    assert_eq!(
        refused.map_err(|failure| failure.error),
        Err(HostError::Config)
    );
    assert_eq!(labelled(&docker, &environment).await?, (0, 0));
    Ok(())
}

#[tokio::test]
async fn local_pair_removal_leaves_another_pair_of_the_environment_intact() -> TestResult<()> {
    let Some(fixture) = Fixture::new().await? else {
        return Ok(());
    };
    let docker = &fixture.docker;
    let both = async {
        let first = Box::pin(start_pair(docker, &fixture.request())).await?;
        let second = Box::pin(start_pair(docker, &fixture.request())).await?;
        let before = labelled(docker, &fixture.environment).await?;
        let removed_first = remove_pair(docker, &first).await?;
        let between = labelled(docker, &fixture.environment).await?;
        let second_running = docker
            .inspect_container(second.runner_id(), None)
            .await?
            .state
            .and_then(|state| state.running);
        let removed_second = remove_pair(docker, &second).await?;
        let after = labelled(docker, &fixture.environment).await?;
        TestResult::Ok((
            before,
            removed_first,
            between,
            second_running,
            removed_second,
            after,
        ))
    }
    .await;
    fixture.finish().await?;
    let (before, removed_first, between, second_running, removed_second, after) = both?;
    let one = Removed {
        containers: 2,
        volumes: 5,
    };
    assert_eq!(before, (4, 10));
    assert_eq!(removed_first, one);
    assert_eq!(between, (2, 5));
    assert_eq!(second_running, Some(true));
    assert_eq!(removed_second, one);
    assert_eq!(after, (0, 0));
    Ok(())
}

#[tokio::test]
async fn two_tests_never_build_the_same_image() -> TestResult<()> {
    let Some(first) = Fixture::new().await? else {
        return Ok(());
    };
    let second = Fixture::new().await;
    let images = second
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map(|second| (first.image.clone(), second.image.clone()));
    first.finish().await?;
    if let Some(second) = second? {
        second.finish().await?;
    }
    let (first_image, second_image) = images.ok_or("the second fixture was not built")?;
    assert_ne!(
        first_image, second_image,
        "a shared image id lets one test's cleanup delete the other's build"
    );
    Ok(())
}

#[tokio::test]
async fn local_pair_start_failure_removes_everything_it_created() -> TestResult<()> {
    let Some(fixture) = Fixture::new().await? else {
        return Ok(());
    };
    let mut failing: LocalPairRequest = fixture.request();
    failing.runner_image = format!("sha256:{}", "b".repeat(64));
    let failed = Box::pin(start_pair(&fixture.docker, &failing)).await;
    let left = labelled(&fixture.docker, &fixture.environment).await;
    fixture.finish().await?;
    assert_eq!(failed, Err(StartFailure::from(HostError::Docker)));
    assert_eq!(left?, (0, 0));
    Ok(())
}

#[tokio::test]
async fn local_pair_refuses_unsafe_requests_without_a_docker_daemon() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("docker.sock");
    drop(std::os::unix::net::UnixListener::bind(&socket)?);
    let docker = velnor_runner_host::connect_unix(&socket.to_string_lossy())?;
    let image = format!("sha256:{}", "a".repeat(64));
    let valid = request("env-one", "velnor-local-net", &image);
    let mut refused = Vec::new();
    for network in [
        "bridge",
        "none",
        "default",
        "host",
        "Host",
        "container:abc",
        "has space",
        " ",
        "",
        "velnor-runner-1-private",
        "velnor-private",
        "VELNOR-runner-1-private",
        "deadbeef",
        "0123456789abcdef",
        "ABCDEF",
    ] {
        refused.push(LocalPairRequest {
            network: network.to_owned(),
            ..valid.clone()
        });
    }
    for image in [
        "",
        "sha256:abc",
        "busybox:latest",
        &format!("sha256:{}", "A".repeat(64)),
        &format!("sha256:{}", "a".repeat(65)),
        &"a".repeat(64),
    ] {
        refused.push(LocalPairRequest {
            runner_image: image.to_owned(),
            ..valid.clone()
        });
        refused.push(LocalPairRequest {
            dind_image: image.to_owned(),
            ..valid.clone()
        });
    }
    refused.push(LocalPairRequest {
        sequence: 0,
        ..valid.clone()
    });
    for budget in 0..4 {
        let mut zero = valid.clone();
        match budget {
            0 => zero.runner_cpu_millicores = 0,
            1 => zero.runner_memory_bytes = 0,
            2 => zero.dind_cpu_millicores = 0,
            _ => zero.dind_memory_bytes = 0,
        }
        refused.push(zero);
    }
    for request in refused {
        let outcome = Box::pin(start_pair(&docker, &request)).await;
        assert_eq!(
            outcome.map_err(|failure| failure.error),
            Err(HostError::Config),
            "{request:?}"
        );
    }
    let accepted = Box::pin(start_pair(&docker, &valid)).await;
    assert_eq!(
        accepted.map_err(|failure| failure.error),
        Err(HostError::Docker)
    );
    Ok(())
}
