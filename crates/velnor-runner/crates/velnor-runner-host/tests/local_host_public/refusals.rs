// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::query_parameters::LogsOptionsBuilder;
use futures_util::TryStreamExt;

use super::authority::{JIT, Run};
use super::scenario::Scenario;
use super::support::{Program, TestResult};
use super::{dispatched, outcome, runner};

fn differing(id_event: Option<&str>, git_ref: Option<&str>, sha: Option<&str>) -> Run {
    Run {
        event: id_event.map(str::to_owned),
        git_ref: git_ref.map(str::to_owned),
        sha: sha.map(str::to_owned),
        ..dispatched("ci")
    }
}

#[tokio::test]
async fn local_job_start_check_refuses_a_run_whose_event_ref_or_sha_differ() -> TestResult<()> {
    let Some(mut scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let plan = scenario.plan(1, 0)?;
    let fake = scenario.fake();
    fake.dispatch("trusted", 1, differing(Some("pull_request"), None, None));
    fake.dispatch(
        "trusted",
        2,
        differing(None, Some("refs/heads/other"), None),
    );
    fake.dispatch("trusted", 3, differing(None, None, Some(&"b".repeat(40))));
    fake.dispatch("trusted", 4, differing(None, None, None));
    scenario.start(plan);
    let run = async {
        let mut outcomes = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for run_id in 1..=4 {
            let live = runner(&scenario, "trusted", &seen).await?;
            seen.push(live.agent.clone());
            let outcome = outcome(&scenario, &live, run_id).await?;
            println!("{} run {run_id}: {outcome}", live.agent);
            outcomes.push(outcome);
        }
        TestResult::Ok(outcomes)
    }
    .await;
    scenario.finish().await?;
    assert_eq!(
        run?,
        [
            "request_mismatch",
            "request_mismatch",
            "request_mismatch",
            "allow:4:1:ci"
        ]
    );
    Ok(())
}

async fn logged(docker: &Docker, container: &str) -> TestResult<String> {
    let options = LogsOptionsBuilder::default()
        .stdout(true)
        .stderr(true)
        .build();
    let chunks: Vec<_> = docker.logs(container, Some(options)).try_collect().await?;
    Ok(chunks.iter().map(ToString::to_string).collect::<String>())
}

#[tokio::test]
async fn local_pair_runner_receives_the_jit_over_its_stdin() -> TestResult<()> {
    let Some(mut scenario) = Scenario::running(Program::Echo).await? else {
        return Ok(());
    };
    let plan = scenario.plan(1, 0)?;
    scenario.start(plan);
    let run = async {
        let live = runner(&scenario, "trusted", &[]).await?;
        let started = Instant::now();
        loop {
            let text = logged(&scenario.fixture.docker, &live.runner).await?;
            if text.contains(JIT) {
                return TestResult::Ok(text);
            }
            if started.elapsed() > Duration::from_secs(120) {
                return Err(format!("the runner never printed the JIT: {text:?}").into());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    .await;
    scenario.finish().await?;
    let text = run?;
    assert_eq!(text.matches(JIT).count(), 1, "{text:?}");
    Ok(())
}
