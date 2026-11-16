// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::models::NetworkCreateRequest;
use bollard::query_parameters::{
    DownloadFromContainerOptions, ListContainersOptions, UploadToContainerOptions,
};
use futures_util::TryStreamExt;
use tokio::task::JoinHandle;
use velnor_resource_probe::job_hook::{Challenge, Decision, SourceRequest, canonical_json};
use velnor_runner_host::HostError;
use velnor_runner_host::local::{Plan, supervise};

use super::authority::{Fake, Served, TOKEN, serve};
use super::support::{Fixture, Program, TestResult};

const WAIT: Duration = Duration::from_secs(240);
const CONTROL: &str = "/opt/velnor/source-proof/control";
const INBOX: &str = "/opt/velnor/source-proof/inbox";

pub(crate) struct Scenario {
    pub(crate) fixture: Fixture,
    pub(crate) served: Served,
    pub(crate) state: tempfile::TempDir,
    pub(crate) fork_network: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), HostError>>>,
}

pub(crate) struct Live {
    pub(crate) runner: String,
    pub(crate) agent: String,
}

impl Scenario {
    pub(crate) async fn new() -> TestResult<Self> {
        Self::with(Fixture::new().await?).await
    }

    pub(crate) async fn running(program: Program) -> TestResult<Self> {
        Self::with(Fixture::running(program).await?).await
    }

    async fn with(fixture: Fixture) -> TestResult<Self> {
        let served = serve(fixture.docker.clone(), &fixture.environment).await?;
        let fork_network = format!("{}-fork", fixture.network);
        fixture.guard.track_network(&fork_network);
        fixture
            .docker
            .create_network(NetworkCreateRequest {
                name: fork_network.clone(),
                ..Default::default()
            })
            .await?;
        Ok(Self {
            fixture,
            served,
            state: tempfile::tempdir()?,
            fork_network,
            stop: None,
            task: None,
        })
    }

    pub(crate) fn fake(&self) -> &Fake {
        &self.served.fake
    }

    pub(crate) fn plan(&self, trusted: u32, untrusted: u32) -> TestResult<Plan> {
        let json = format!(
            "{{\"environment\":\"{}\",\"authority\":\"http://{}\",\"token\":\"{TOKEN}\",\"docker\":\"unix:///var/run/docker.sock\",\"platform\":\"{}\",\"runner_image\":\"{}\",\"dind_image\":\"{}\",\"limits\":{{\"runner_cpu_millicores\":1500,\"runner_memory_bytes\":1073741824,\"dind_cpu_millicores\":500,\"dind_memory_bytes\":1073741824}},\"sides\":[{{\"side\":\"trusted\",\"network\":\"{}\",\"pairs\":{trusted}}},{{\"side\":\"untrusted\",\"network\":\"{}\",\"pairs\":{untrusted}}}]}}",
            self.fixture.environment,
            self.served.address,
            super::support::platform(),
            self.fixture.image,
            self.fixture.image,
            self.fixture.network,
            self.fork_network,
        );
        Ok(Plan::parse(json.as_bytes())?)
    }

    pub(crate) fn start(&mut self, plan: Plan) {
        let (stop, wait) = tokio::sync::oneshot::channel::<()>();
        let docker = self.fixture.docker.clone();
        let state = self.state.path().to_path_buf();
        self.stop = Some(stop);
        self.task = Some(tokio::spawn(async move {
            supervise(docker, plan, &state, async move {
                let _stopped = wait.await;
            })
            .await
        }));
    }

    pub(crate) async fn stop(&mut self) -> TestResult<Result<(), HostError>> {
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        match self.task.take() {
            Some(task) => Ok(tokio::time::timeout(WAIT, task).await??),
            None => Ok(Ok(())),
        }
    }

    pub(crate) fn decisions(&self) -> Vec<serde_json::Value> {
        let path = self.state.path().join("decisions.jsonl");
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    pub(crate) async fn finish(mut self) -> TestResult<()> {
        let stopped = self.stop().await;
        self.fixture.finish().await?;
        stopped??;
        Ok(())
    }
}

pub(crate) async fn live_runners(
    docker: &Docker,
    environment: &str,
    side: &str,
) -> TestResult<Vec<Live>> {
    let filters = HashMap::from([(
        "label".to_owned(),
        vec![
            format!("velnor-local={environment}"),
            format!("velnor-local.side={side}"),
            "velnor-local.role=runner".to_owned(),
        ],
    )]);
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: false,
            filters: Some(filters),
            ..Default::default()
        }))
        .await?;
    Ok(containers
        .into_iter()
        .filter_map(|container| {
            Some(Live {
                runner: container.id?,
                agent: container.labels?.get("velnor-local.agent")?.clone(),
            })
        })
        .collect())
}

pub(crate) async fn wait_for_runner(
    docker: &Docker,
    environment: &str,
    side: &str,
    unlike: &[&str],
) -> TestResult<Live> {
    let started = Instant::now();
    loop {
        let found = live_runners(docker, environment, side)
            .await?
            .into_iter()
            .find(|live| !unlike.contains(&live.agent.as_str()));
        if let Some(live) = found
            && read_challenge(docker, &live.runner).await.is_ok()
        {
            return Ok(live);
        }
        if started.elapsed() > WAIT {
            return Err("no new runner with a challenge".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn download(docker: &Docker, container: &str, path: &str) -> TestResult<Vec<u8>> {
    let chunks: Vec<_> = docker
        .download_from_container(
            container,
            Some(DownloadFromContainerOptions {
                path: path.to_owned(),
            }),
        )
        .try_collect()
        .await?;
    let archive: Vec<u8> = chunks.into_iter().flatten().collect();
    let mut archive = tar::Archive::new(archive.as_slice());
    let mut entry = archive.entries()?.next().ok_or("empty archive")??;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)?;
    Ok(bytes)
}

pub(crate) async fn read_challenge(docker: &Docker, runner: &str) -> TestResult<Challenge> {
    let bytes = download(docker, runner, &format!("{CONTROL}/challenge.json")).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(crate) async fn wait_for_decision(docker: &Docker, runner: &str) -> TestResult<Decision> {
    let started = Instant::now();
    loop {
        if let Ok(bytes) = download(docker, runner, &format!("{CONTROL}/decision.json")).await {
            return Ok(serde_json::from_slice(&bytes)?);
        }
        if started.elapsed() > Duration::from_secs(120) {
            return Err("no decision within the hook's 120 s".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub(crate) fn request(challenge: &Challenge, run_id: u64, job: &str) -> TestResult<Vec<u8>> {
    let request = SourceRequest {
        schema_version: 1,
        challenge: challenge.nonce.clone(),
        reservation_id: challenge.reservation_id,
        intent_id: challenge.intent_id,
        repository_id: 1,
        repository_name: "tailrocks/velnor-lab".to_owned(),
        event_name: "push".to_owned(),
        event_ref: "refs/heads/main".to_owned(),
        tested_sha: "a".repeat(40),
        run_id,
        run_attempt: 1,
        job_key: job.to_owned(),
        workflow_ref: "tailrocks/velnor-lab/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: "b".repeat(40),
        base_ref: None,
        base_sha: None,
        head_ref: None,
        head_sha: None,
        pull_request_number: None,
    };
    Ok(canonical_json(&request, 16 * 1024, "test_request")?)
}

pub(crate) async fn submit(docker: &Docker, runner: &str, bytes: &[u8]) -> TestResult<()> {
    let mut header = tar::Header::new_ustar();
    header.set_path("request.json")?;
    header.set_uid(1000);
    header.set_gid(1000);
    header.set_mode(0o600);
    header.set_size(u64::try_from(bytes.len())?);
    header.set_cksum();
    let mut archive = tar::Builder::new(Vec::new());
    archive.append(&header, bytes)?;
    docker
        .upload_to_container(
            runner,
            Some(UploadToContainerOptions {
                path: INBOX.to_owned(),
                ..Default::default()
            }),
            bollard::body_full(archive.into_inner()?.into()),
        )
        .await?;
    Ok(())
}

pub(crate) async fn finish_job(docker: &Docker, runner: &str) -> TestResult<()> {
    docker
        .kill_container(
            runner,
            None::<bollard::query_parameters::KillContainerOptions>,
        )
        .await?;
    Ok(())
}
