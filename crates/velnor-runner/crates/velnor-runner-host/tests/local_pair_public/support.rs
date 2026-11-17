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

#[path = "endpoint.rs"]
mod endpoint;
pub(crate) use endpoint::Endpoint;

#[path = "guard.rs"]
mod guard;
#[path = "program.rs"]
mod program;
pub(crate) use guard::Guard;
pub(crate) use program::Program;

pub(crate) type TestResult<T> = Result<T, Box<dyn Error>>;

pub(crate) fn docker() -> TestResult<Option<Docker>> {
    Endpoint::selected()?
        .map(|endpoint| endpoint.docker())
        .transpose()
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

pub(crate) fn job_hook_binary() -> TestResult<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("VELNOR_TEST_JOB_HOOK_BINARY") {
        return Ok(path.into());
    }
    if cfg!(target_os = "linux") {
        return Ok(env!("CARGO_BIN_EXE_velnor-local-test-job-hook").into());
    }
    Err("set VELNOR_TEST_JOB_HOOK_BINARY to a Linux velnor-job-start-hook binary".into())
}

fn append(
    context: &mut tar::Builder<Vec<u8>>,
    path: &str,
    mode: u32,
    bytes: &[u8],
) -> TestResult<()> {
    let mut header = tar::Header::new_ustar();
    header.set_path(path)?;
    header.set_size(u64::try_from(bytes.len())?);
    header.set_mode(mode);
    header.set_cksum();
    context.append(&header, bytes)?;
    Ok(())
}

pub(crate) async fn build_program(
    docker: &Docker,
    tag: &str,
    program: Program,
) -> TestResult<String> {
    let mut context = tar::Builder::new(Vec::new());
    let base = if cfg!(target_arch = "aarch64") {
        "FROM ubuntu:26.04@sha256:7e7957462d7b6db141de67623f46135d82a6bc131bc2ff369aedfccfa91c0a1b"
    } else {
        include_str!("../../../../../../images/dind/Dockerfile")
            .lines()
            .find(|line| line.starts_with("FROM ubuntu:"))
            .ok_or("dind image has no Ubuntu base")?
    };
    let mut dockerfile = format!("{base}\nCOPY program /program\nCMD [\"/program\"]\n");
    if cfg!(target_os = "linux") || std::env::var_os("VELNOR_TEST_JOB_HOOK_BINARY").is_some() {
        append(
            &mut context,
            "job-hook",
            0o755,
            &std::fs::read(job_hook_binary()?)?,
        )?;
        dockerfile.push_str("COPY job-hook /velnor-job-start-hook\n");
    }
    append(&mut context, "Dockerfile", 0o644, dockerfile.as_bytes())?;
    append(&mut context, "program", 0o755, &program::elf(program))?;
    let options = BuildImageOptions {
        dockerfile: "Dockerfile".to_owned(),
        t: Some(tag.to_owned()),
        platform: platform().to_owned(),
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

pub(crate) fn untag_only() -> RemoveImageOptions {
    RemoveImageOptions {
        noprune: true,
        ..Default::default()
    }
}

pub(crate) fn request(environment: &str, network: &str, image: &str) -> LocalPairRequest {
    LocalPairRequest {
        environment: environment.to_owned(),
        side: "trusted".to_owned(),
        agent: format!("{environment}-agent"),
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
    pub(crate) endpoint: Endpoint,
    pub(crate) environment: String,
    pub(crate) network: String,
    pub(crate) image: String,
    pub(crate) guard: Guard,
}

impl Fixture {
    pub(crate) async fn new() -> TestResult<Option<Self>> {
        Self::running(Program::Sleep).await
    }

    pub(crate) async fn running(program: Program) -> TestResult<Option<Self>> {
        let Some(endpoint) = Endpoint::selected()? else {
            return Ok(None);
        };
        let docker = endpoint.docker()?;
        let suffix = suffix()?;
        let tag = format!("velnor-local-test-sleeper:{suffix}");
        let network = format!("velnor-local-test-{suffix}");
        let image = build_program(&docker, &tag, program).await?;
        docker
            .create_network(NetworkCreateRequest {
                name: network.clone(),
                ..Default::default()
            })
            .await?;
        Ok(Some(Self {
            endpoint,
            docker: docker.clone(),
            environment: format!("test-{suffix}"),
            guard: Guard::new(docker, &format!("test-{suffix}"), &tag, &network),
            network,
            image,
        }))
    }

    pub(crate) fn request(&self) -> LocalPairRequest {
        request(&self.environment, &self.network, &self.image)
    }

    pub(crate) fn image_tag(&self) -> &str {
        self.guard.image_tag()
    }

    pub(crate) async fn finish(mut self) -> TestResult<()> {
        self.guard.finish().await
    }
}
