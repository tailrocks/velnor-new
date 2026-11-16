// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::path::Path;

use bollard::models::{ContainerCreateBody, VolumeCreateRequest};
use bollard::query_parameters::{CreateContainerOptions, RemoveContainerOptions};
use serde_json::{Value, json};
use velnor_runner_host::HostError;
use velnor_runner_host::local::supervise;

use super::proxy::{Faults, Proxy};
use super::scenario::Scenario;
use super::support::TestResult;

async fn container(scenario: &Scenario, environment: &str, pair: bool) -> TestResult<String> {
    let mut labels = HashMap::from([("velnor-local".to_owned(), environment.to_owned())]);
    if pair {
        labels.insert(
            "velnor-local.pair".to_owned(),
            format!("{environment}-orphan"),
        );
    }
    Ok(scenario
        .fixture
        .docker
        .create_container(
            None::<CreateContainerOptions>,
            ContainerCreateBody {
                image: Some(scenario.fixture.image.clone()),
                labels: Some(labels),
                ..Default::default()
            },
        )
        .await?
        .id)
}

fn discarded(result: Result<(), bollard::errors::Error>) -> TestResult<()> {
    match result {
        Ok(())
        | Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn discard(scenario: &Scenario, id: &str) -> TestResult<()> {
    discarded(
        scenario
            .fixture
            .docker
            .remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await,
    )
}

async fn exists(scenario: &Scenario, id: &str) -> bool {
    scenario
        .fixture
        .docker
        .inspect_container(id, None)
        .await
        .is_ok()
}

fn journal(state: &Path) -> TestResult<Vec<Value>> {
    std::fs::read_to_string(state.join("cleanup.jsonl"))?
        .lines()
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

async fn sweep(scenario: &Scenario, docker: bollard::Docker) -> TestResult<Result<(), HostError>> {
    let plan = scenario.plan(1, 0)?;
    Ok(supervise(docker, plan, scenario.state.path(), std::future::ready(())).await)
}

#[tokio::test]
async fn supervisor_sweep_audits_exact_filter_and_actual_removals() -> TestResult<()> {
    let Some(scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let environment = scenario.fixture.environment.clone();
    let orphan = container(&scenario, &environment, true).await?;
    let service = container(&scenario, &environment, false).await?;
    let neighbour = container(&scenario, &format!("{environment}-other"), true).await?;
    let volume = scenario
        .fixture
        .docker
        .create_volume(VolumeCreateRequest {
            labels: Some(HashMap::from([
                ("velnor-local".to_owned(), environment.clone()),
                ("velnor-local.pair".to_owned(), "orphan".to_owned()),
            ])),
            ..Default::default()
        })
        .await?
        .name;
    let swept = sweep(&scenario, scenario.fixture.docker.clone()).await?;
    let gone = !exists(&scenario, &orphan).await;
    let volume_gone = scenario
        .fixture
        .docker
        .inspect_volume(&volume)
        .await
        .is_err();
    let kept = exists(&scenario, &service).await && exists(&scenario, &neighbour).await;
    let records = journal(scenario.state.path());
    for id in [&orphan, &service, &neighbour] {
        discard(&scenario, id).await?;
    }
    discarded(
        scenario
            .fixture
            .docker
            .remove_volume(
                &volume,
                None::<bollard::query_parameters::RemoveVolumeOptions>,
            )
            .await,
    )?;
    scenario.finish().await?;
    assert_eq!(swept, Ok(()));
    assert!(gone && volume_gone && kept);
    let records = records?;
    for record in &records {
        assert_eq!(
            record["filters"],
            json!({"label": [format!("velnor-local={environment}"), "velnor-local.pair"]})
        );
    }
    for (kind, id) in [("container", orphan), ("volume", volume)] {
        let attempted = records.iter().position(|line| {
            line["event"] == "intent" && line["resource"] == kind && line["id"] == id
        });
        let removed = records.iter().position(|line| {
            line["event"] == "removed" && line["resource"] == kind && line["id"] == id
        });
        assert!(attempted.is_some() && removed.is_some() && attempted < removed);
    }
    Ok(())
}

#[tokio::test]
async fn supervisor_sweep_preserves_partial_success_and_appends_retry() -> TestResult<()> {
    let Some(scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let environment = scenario.fixture.environment.clone();
    let first = container(&scenario, &environment, true).await?;
    let second = container(&scenario, &environment, true).await?;
    let proxy = Proxy::start(&scenario.fixture.endpoint, Faults::fail_second_removal())?;
    let failed = sweep(&scenario, proxy.docker.clone()).await?;
    let before = journal(scenario.state.path())?;
    let removed: Vec<_> = before
        .iter()
        .filter(|line| line["event"] == "removed")
        .collect();
    let gone = if let Some(line) = removed.first() {
        !exists(&scenario, line["id"].as_str().unwrap_or_default()).await
    } else {
        false
    };
    let survivors = usize::from(exists(&scenario, &first).await)
        + usize::from(exists(&scenario, &second).await);
    let retry = sweep(&scenario, proxy.docker.clone()).await?;
    let after = journal(scenario.state.path())?;
    discard(&scenario, &first).await?;
    discard(&scenario, &second).await?;
    scenario.finish().await?;
    assert_eq!(failed, Err(HostError::Docker));
    assert_eq!(removed.len(), 1);
    assert!(gone);
    assert_eq!(survivors, 1);
    assert_eq!(retry, Ok(()));
    assert_eq!(&after[..before.len()], &before);
    for id in [first, second] {
        assert_eq!(
            after
                .iter()
                .filter(|line| line["event"] == "removed" && line["id"] == id)
                .count(),
            1
        );
    }
    Ok(())
}

#[tokio::test]
async fn supervisor_sweep_refuses_deletion_without_a_writable_audit() -> TestResult<()> {
    let Some(scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let orphan = container(&scenario, &scenario.fixture.environment, true).await?;
    std::fs::create_dir(scenario.state.path().join("cleanup.jsonl"))?;
    let failed = sweep(&scenario, scenario.fixture.docker.clone()).await?;
    let kept = exists(&scenario, &orphan).await;
    discard(&scenario, &orphan).await?;
    scenario.finish().await?;
    assert_eq!(failed, Err(HostError::Cleanup));
    assert!(kept);
    Ok(())
}
