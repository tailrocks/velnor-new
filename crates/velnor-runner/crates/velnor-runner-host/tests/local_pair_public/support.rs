// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn append_root_file(
    context: &mut tar::Builder<Vec<u8>>,
    source: &Path,
    destination: &Path,
) -> TestResult<()> {
    let destination = format!(
        "rootfs/{}",
        destination
            .to_str()
            .ok_or("non-UTF-8 runtime path")?
            .trim_start_matches('/')
    );
    append(context, &destination, 0o755, &std::fs::read(source)?)
}

fn runtime_dependencies(binary: &Path) -> TestResult<Vec<PathBuf>> {
    let output = Command::new("ldd").arg(binary).output()?;
    if !output.status.success() {
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if diagnostic.contains("not a dynamic executable")
            || diagnostic.contains("statically linked")
        {
            return Ok(Vec::new());
        }
        return Err(format!("ldd failed for {}", binary.display()).into());
    }
    let output = String::from_utf8(output.stdout)?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let dependency = line
                .split_once("=>")
                .map_or(line, |(_, dependency)| dependency)
                .split_whitespace()
                .next()?;
            dependency
                .starts_with('/')
                .then(|| PathBuf::from(dependency))
        })
        .collect())
}

fn append_runtime(context: &mut tar::Builder<Vec<u8>>, job_hook: &Path) -> TestResult<()> {
    let binaries = vec![
        PathBuf::from("/bin/sh"),
        PathBuf::from("/bin/sync"),
        PathBuf::from("/bin/mv"),
        PathBuf::from("/usr/bin/setsid"),
    ];
    let mut dependencies = Vec::new();
    for binary in binaries
        .iter()
        .map(PathBuf::as_path)
        .chain(std::iter::once(job_hook))
    {
        dependencies.extend(runtime_dependencies(binary)?);
    }
    for binary in binaries {
        append_root_file(context, &binary, &binary)?;
    }
    dependencies.sort();
    dependencies.dedup();
    for dependency in dependencies {
        append_root_file(context, &dependency, &dependency)?;
    }
    Ok(())
}

fn append_rootfs(
    context: &mut tar::Builder<Vec<u8>>,
    root: &Path,
    directory: &Path,
) -> TestResult<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(format!("runtime root contains symlink {}", path.display()).into());
        }
        if file_type.is_dir() {
            append_rootfs(context, root, &path)?;
        } else {
            let relative = path.strip_prefix(root)?;
            let destination = format!(
                "rootfs/{}",
                relative.to_str().ok_or("non-UTF-8 runtime path")?
            );
            append(context, &destination, 0o755, &std::fs::read(path)?)?;
        }
    }
    Ok(())
}

pub(crate) async fn build_program(
    docker: &Docker,
    tag: &str,
    program: Program,
) -> TestResult<String> {
    let mut context = tar::Builder::new(Vec::new());
    // Every test image starts with a layer no other test has: layers that are
    // byte-identical share one snapshot chain on the containerd image store, so
    // another test's image removal can delete the parent this build is on.
    append(&mut context, "nonce", 0o644, tag.as_bytes())?;
    let mut dockerfile = "FROM scratch\nCOPY nonce /velnor-test-nonce\n".to_owned();
    if cfg!(target_os = "linux") || std::env::var_os("VELNOR_TEST_JOB_HOOK_BINARY").is_some() {
        let binary = job_hook_binary()?;
        append(&mut context, "job-hook", 0o755, &std::fs::read(&binary)?)?;
        if cfg!(target_os = "linux") {
            append_runtime(&mut context, &binary)?;
        } else if let Some(rootfs) = std::env::var_os("VELNOR_TEST_JOB_HOOK_ROOTFS") {
            let root = PathBuf::from(rootfs);
            append_rootfs(&mut context, &root, &root)?;
        } else {
            return Err("set VELNOR_TEST_JOB_HOOK_ROOTFS to a Linux root filesystem".into());
        }
        dockerfile.push_str("COPY rootfs/ /\nCOPY job-hook /velnor-job-start-hook\n");
    }
    dockerfile.push_str("COPY program /program\nCMD [\"/program\"]\n");
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
