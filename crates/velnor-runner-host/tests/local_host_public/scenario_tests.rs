// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::Docker;
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures_util::TryStreamExt;

use super::{Scenario, TestResult, download, finish_job, wait_for_decision, wait_for_runner};
use crate::authority::Run;
use crate::publication_proxy::PublicationProxy;
use velnor_resource_probe::job_hook::Decision;

const CONTROL: &str = "/opt/velnor/source-proof/control";
const WAIT: Duration = Duration::from_secs(20);

struct Guest {
    id: String,
    output:
        tokio::task::JoinHandle<Result<Vec<bollard::container::LogOutput>, bollard::errors::Error>>,
}

async fn guest(docker: &Docker, runner: &str, run_id: u64) -> TestResult<Guest> {
    let run = format!("GITHUB_RUN_ID={run_id}");
    let env = [
        "GITHUB_REPOSITORY=tailrocks/velnor-lab",
        "GITHUB_REPOSITORY_ID=1",
        "GITHUB_EVENT_NAME=push",
        "GITHUB_REF=refs/heads/main",
        run.as_str(),
        "GITHUB_RUN_ATTEMPT=1",
        "GITHUB_JOB=ci",
        "GITHUB_WORKFLOW_REF=tailrocks/velnor-lab/.github/workflows/ci.yml@refs/heads/main",
        "GITHUB_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "GITHUB_WORKFLOW_SHA=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    ];
    let created = docker
        .create_exec(
            runner,
            CreateExecOptions {
                user: Some("1000"),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec!["/usr/bin/setsid", "--wait", "/velnor-job-start-hook"]),
                env: Some(env.to_vec()),
                ..Default::default()
            },
        )
        .await?;
    let started = docker.start_exec(&created.id, None).await?;
    let StartExecResults::Attached { output, .. } = started else {
        return Err("guest hook did not attach".into());
    };
    Ok(Guest {
        id: created.id,
        output: tokio::spawn(output.try_collect()),
    })
}

async fn staged_file(docker: &Docker, runner: &str, name: &str, expected: &[u8]) -> TestResult<()> {
    tokio::time::timeout(WAIT, async {
        loop {
            if let Ok(bytes) = download(docker, runner, &format!("{CONTROL}/{name}")).await
                && bytes == expected
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    Ok(())
}

async fn race(
    scenario: &mut Scenario,
    proxy: &mut PublicationProxy,
    allowed: bool,
    seen: &mut Vec<String>,
) -> TestResult<()> {
    let run_id = u64::try_from(seen.len())? + 1;
    scenario.fake().dispatch(
        "trusted",
        run_id,
        Run {
            job: "ci".to_owned(),
            ledger: allowed.then(|| "1-push".to_owned()),
            refusal: (!allowed).then(|| "not_dispatched".to_owned()),
            ..Run::default()
        },
    );
    let previous: Vec<&str> = seen.iter().map(String::as_str).collect();
    let live = wait_for_runner(
        &scenario.fixture.docker,
        &scenario.fixture.environment,
        "trusted",
        &previous,
    )
    .await?;
    seen.push(live.agent);
    let docker = &scenario.fixture.docker;
    let guest = guest(docker, &live.runner, run_id).await?;
    for _ in 0..2 {
        let stage = tokio::time::timeout(WAIT, proxy.stages.recv())
            .await?
            .ok_or("publication proxy stopped")?;
        staged_file(docker, &live.runner, &stage.name, &stage.bytes).await?;
        if download(docker, &live.runner, &format!("{CONTROL}/decision.json"))
            .await
            .is_ok()
        {
            return Err("an empty or partial decision became visible".into());
        }
        let inspection = docker.inspect_exec(&guest.id).await?;
        if inspection.running != Some(true) {
            let output = guest.output.await??;
            return Err(
                format!("guest exited before publication: {inspection:?}: {output:?}").into(),
            );
        }
        stage.resume.send(()).map_err(|()| "publisher stopped")?;
    }
    let decision = wait_for_decision(docker, &live.runner).await?;
    assert_eq!(decision.allow, allowed);
    let output = tokio::time::timeout(WAIT, guest.output).await???;
    let output = output
        .into_iter()
        .map(|line| line.to_string())
        .collect::<String>();
    let exit = docker.inspect_exec(&guest.id).await?.exit_code;
    if allowed {
        assert_eq!(exit, Some(0), "{output}");
        assert!(output.is_empty(), "{output}");
    } else {
        assert_ne!(exit, Some(0), "{output}");
        assert!(output.contains("job_hook_decision_mismatch"), "{output}");
        assert!(!output.contains("job_hook_json"), "{output}");
    }
    finish_job(docker, &live.runner).await?;
    Ok(())
}

async fn replacement(
    scenario: &mut Scenario,
    proxy: &mut PublicationProxy,
    seen: &mut Vec<String>,
) -> TestResult<()> {
    let run_id = u64::try_from(seen.len())? + 1;
    scenario.fake().dispatch(
        "trusted",
        run_id,
        Run {
            job: "ci".to_owned(),
            ledger: Some("1-push".to_owned()),
            ..Run::default()
        },
    );
    let previous: Vec<&str> = seen.iter().map(String::as_str).collect();
    let live = wait_for_runner(
        &scenario.fixture.docker,
        &scenario.fixture.environment,
        "trusted",
        &previous,
    )
    .await?;
    seen.push(live.agent);
    let docker = &scenario.fixture.docker;
    let challenge = super::read_challenge(docker, &live.runner).await?;
    let old = crate::previous_decision::seed(&scenario.fixture, &live.runner, &challenge).await?;
    super::submit(
        docker,
        &live.runner,
        &super::request(&challenge, run_id, "ci")?,
    )
    .await?;
    let mut expected = None;
    for _ in 0..2 {
        let stage = tokio::time::timeout(WAIT, proxy.stages.recv())
            .await?
            .ok_or("publication proxy stopped")?;
        staged_file(docker, &live.runner, &stage.name, &stage.bytes).await?;
        if expected.is_none() {
            expected = Some(serde_json::from_slice::<Decision>(&stage.document)?);
        }
        let observed = download(docker, &live.runner, &format!("{CONTROL}/decision.json")).await?;
        if observed != old {
            return Err("partial replacement overwrote the previous complete decision".into());
        }
        stage.resume.send(()).map_err(|()| "publisher stopped")?;
    }
    tokio::time::timeout(WAIT, async {
        loop {
            let observed =
                download(docker, &live.runner, &format!("{CONTROL}/decision.json")).await?;
            let decision: Decision = serde_json::from_slice(&observed)?;
            if Some(decision) == expected {
                return TestResult::Ok(());
            }
            if observed != old {
                return Err("replacement exposed neither complete document".into());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await??;
    finish_job(docker, &live.runner).await?;
    Ok(())
}

#[tokio::test]
async fn atomic_decision_publication_keeps_empty_and_partial_data_from_guest_outcomes()
-> TestResult<()> {
    let Some(mut scenario) = Scenario::new_with_runtime().await? else {
        return Ok(());
    };
    let _binary = crate::support::job_hook_binary()?;
    let mut proxy = PublicationProxy::start(&scenario.fixture.endpoint)?;
    let plan = scenario.plan(1, 0)?;
    scenario.start_on(proxy.docker.clone(), plan);
    let result = async {
        let mut seen = Vec::new();
        let repetitions = std::env::var("VELNOR_PUBLICATION_REPETITIONS")
            .map_or(Ok(1), |value| value.parse::<usize>())?;
        if repetitions == 0 {
            return Err("VELNOR_PUBLICATION_REPETITIONS must be positive".into());
        }
        for _ in 0..repetitions {
            race(&mut scenario, &mut proxy, true, &mut seen).await?;
            race(&mut scenario, &mut proxy, false, &mut seen).await?;
            replacement(&mut scenario, &mut proxy, &mut seen).await?;
        }
        TestResult::Ok(())
    }
    .await;
    scenario.finish().await?;
    result
}
