// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::error::Error;
use std::fmt::Write as _;

use bollard::Docker;
use bollard::models::{ContainerInspectResponse, NetworkCreateRequest};
use bollard::query_parameters::{
    BuildImageOptions, ListContainersOptions, ListVolumesOptions, RemoveImageOptions,
};
use futures_util::StreamExt;
use velnor_runner_host::HostError;
use velnor_runner_host::local::{
    ENVIRONMENT_LABEL, LocalPairRequest, Removed, remove_pair, start_pair,
};

type TestResult<T> = Result<T, Box<dyn Error>>;

const BASE: &str =
    "busybox:1.37.0@sha256:a7dbc319b1438115a207e11930dc4bb3e643dcb3ec0bdd694bb36cd8fad289ec";
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

fn docker() -> TestResult<Docker> {
    let socket =
        std::env::var("DOCKER_HOST").unwrap_or_else(|_| "unix:///var/run/docker.sock".to_owned());
    Ok(velnor_runner_host::connect_unix(&socket)?)
}

fn suffix() -> TestResult<String> {
    let mut bytes = [0_u8; 6];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    let mut text = String::new();
    for byte in bytes {
        write!(text, "{byte:02x}")?;
    }
    Ok(text)
}

fn platform() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "linux/arm64"
    } else {
        "linux/amd64"
    }
}

async fn build_sleeper(docker: &Docker, tag: &str) -> TestResult<String> {
    let dockerfile = format!("FROM {BASE}\nCMD [\"sleep\", \"600\"]\n");
    let mut header = tar::Header::new_ustar();
    header.set_path("Dockerfile")?;
    header.set_size(u64::try_from(dockerfile.len())?);
    header.set_mode(0o644);
    header.set_cksum();
    let mut context = tar::Builder::new(Vec::new());
    context.append(&header, dockerfile.as_bytes())?;
    let options = BuildImageOptions {
        dockerfile: "Dockerfile".to_owned(),
        t: Some(tag.to_owned()),
        rm: true,
        ..Default::default()
    };
    let mut build = docker.build_image(
        options,
        None,
        Some(bollard::body_full(context.into_inner()?.into())),
    );
    while let Some(step) = build.next().await {
        if let Some(error) = step?.error_detail {
            return Err(format!("build failed: {error:?}").into());
        }
    }
    docker
        .inspect_image(tag)
        .await?
        .id
        .ok_or_else(|| "built image has no id".into())
}

fn request(environment: &str, network: &str, image: &str) -> LocalPairRequest {
    LocalPairRequest {
        environment: environment.to_owned(),
        network: network.to_owned(),
        platform: platform().to_owned(),
        runner_image: image.to_owned(),
        dind_image: image.to_owned(),
        sequence: 1,
        runner_cpu_millicores: 1500,
        runner_memory_bytes: 1_073_741_824,
        dind_cpu_millicores: 500,
        dind_memory_bytes: 1_073_741_824,
    }
}

async fn labelled(docker: &Docker, environment: &str) -> TestResult<(usize, usize)> {
    let filters = HashMap::from([(
        "label".to_owned(),
        vec![format!("{ENVIRONMENT_LABEL}={environment}")],
    )]);
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters: Some(filters.clone()),
            ..Default::default()
        }))
        .await?;
    let volumes = docker
        .list_volumes(Some(ListVolumesOptions {
            filters: Some(filters),
        }))
        .await?;
    Ok((containers.len(), volumes.volumes.unwrap_or_default().len()))
}

struct Observed {
    dind: ContainerInspectResponse,
    runner: ContainerInspectResponse,
    dind_id: String,
    removed: Removed,
    left: (usize, usize),
}

async fn observe_pair(
    docker: &Docker,
    environment: &str,
    network: &str,
    image: &str,
) -> TestResult<Observed> {
    let pair = Box::pin(start_pair(docker, &request(environment, network, image))).await?;
    let dind = docker.inspect_container(pair.dind_id(), None).await;
    let runner = docker.inspect_container(pair.runner_id(), None).await;
    let removed = remove_pair(docker, &pair).await?;
    Ok(Observed {
        dind: dind?,
        runner: runner?,
        dind_id: pair.dind_id().to_owned(),
        removed,
        left: labelled(docker, environment).await?,
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
    let docker = docker()?;
    let suffix = suffix()?;
    let environment = format!("test-{suffix}");
    let network = format!("velnor-local-test-{suffix}");
    let tag = format!("velnor-local-test-sleeper:{suffix}");
    let image = build_sleeper(&docker, &tag).await?;
    docker
        .create_network(NetworkCreateRequest {
            name: network.clone(),
            ..Default::default()
        })
        .await?;
    let observed = Box::pin(observe_pair(&docker, &environment, &network, &image)).await;
    docker.remove_network(&network).await?;
    docker
        .remove_image(&tag, None::<RemoveImageOptions>, None)
        .await?;
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
    Ok(())
}

#[tokio::test]
async fn local_pair_refuses_the_host_network_before_creating_anything() -> TestResult<()> {
    let docker = docker()?;
    let environment = format!("test-{}", suffix()?);
    let refused = Box::pin(start_pair(
        &docker,
        &request(&environment, "host", &format!("sha256:{}", "a".repeat(64))),
    ))
    .await;
    assert_eq!(refused, Err(HostError::Config));
    assert_eq!(labelled(&docker, &environment).await?, (0, 0));
    Ok(())
}
