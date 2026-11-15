// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::error::Error;
use std::fmt::Write as _;

use bollard::Docker;
use bollard::models::NetworkCreateRequest;
use bollard::query_parameters::{
    BuildImageOptions, ListContainersOptions, ListVolumesOptions, RemoveImageOptions,
};
use futures_util::StreamExt;
use velnor_runner_host::local::{ENVIRONMENT_LABEL, LocalPairRequest};

pub(crate) type TestResult<T> = Result<T, Box<dyn Error>>;

const BASE: &str =
    "busybox:1.37.0@sha256:a7dbc319b1438115a207e11930dc4bb3e643dcb3ec0bdd694bb36cd8fad289ec";
pub(crate) fn docker() -> TestResult<Docker> {
    let socket =
        std::env::var("DOCKER_HOST").unwrap_or_else(|_| "unix:///var/run/docker.sock".to_owned());
    Ok(velnor_runner_host::connect_unix(&socket)?)
}

pub(crate) fn suffix() -> TestResult<String> {
    let mut bytes = [0_u8; 6];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    let mut text = String::new();
    for byte in bytes {
        write!(text, "{byte:02x}")?;
    }
    Ok(text)
}

pub(crate) fn platform() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "linux/arm64"
    } else {
        "linux/amd64"
    }
}

pub(crate) async fn build_sleeper(docker: &Docker, tag: &str) -> TestResult<String> {
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

pub(crate) fn request(environment: &str, network: &str, image: &str) -> LocalPairRequest {
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

pub(crate) async fn labelled(docker: &Docker, environment: &str) -> TestResult<(usize, usize)> {
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

pub(crate) struct Fixture {
    pub(crate) docker: Docker,
    pub(crate) environment: String,
    pub(crate) network: String,
    pub(crate) image: String,
    tag: String,
}

impl Fixture {
    pub(crate) async fn new() -> TestResult<Self> {
        let docker = docker()?;
        let suffix = suffix()?;
        let tag = format!("velnor-local-test-sleeper:{suffix}");
        let network = format!("velnor-local-test-{suffix}");
        let image = build_sleeper(&docker, &tag).await?;
        docker
            .create_network(NetworkCreateRequest {
                name: network.clone(),
                ..Default::default()
            })
            .await?;
        Ok(Self {
            docker,
            environment: format!("test-{suffix}"),
            network,
            image,
            tag,
        })
    }

    pub(crate) fn request(&self) -> LocalPairRequest {
        request(&self.environment, &self.network, &self.image)
    }

    pub(crate) async fn finish(self) -> TestResult<()> {
        self.docker.remove_network(&self.network).await?;
        self.docker
            .remove_image(&self.tag, None::<RemoveImageOptions>, None)
            .await?;
        Ok(())
    }
}
