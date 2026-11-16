// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use bollard::errors::Error;
use bollard::models::{NetworkCreateRequest, VolumeCreateRequest};
use bollard::query_parameters::TagImageOptions;
use velnor_runner_host::local::start_pair;

use super::proxy::{Faults, Proxy};
use super::scenario::{Scenario, finish_job};
use super::support::{Fixture, Guard, TestResult, labelled, suffix};

fn absent<T: std::fmt::Debug>(result: &Result<T, Error>) {
    assert!(
        matches!(
            result,
            Err(Error::DockerResponseServerError {
                status_code: 404,
                ..
            })
        ),
        "{result:?}"
    );
}

async fn assert_removed(docker: &Docker, containers: &[String], networks: &[String]) {
    for id in containers {
        absent(&docker.inspect_container(id, None).await);
    }
    for network in networks {
        absent(&docker.inspect_network(network, None).await);
    }
}

#[tokio::test]
async fn local_fixture_cleanup_removes_owned_endpoints_before_their_networks() -> TestResult<()> {
    let Some(scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let Some(foreign) = Fixture::new().await? else {
        return Ok(());
    };
    let docker = scenario.fixture.docker.clone();
    let trusted = Box::pin(start_pair(&docker, &scenario.fixture.request())).await?;
    let mut request = scenario.fixture.request();
    request.side = "untrusted".to_owned();
    request.network.clone_from(&scenario.fork_network);
    request.agent.push_str("-fork");
    let fork = Box::pin(start_pair(&docker, &request)).await?;
    let other = Box::pin(start_pair(&docker, &foreign.request())).await?;
    finish_job(&docker, trusted.runner_id()).await?;
    let stopped = docker.inspect_container(trusted.runner_id(), None).await?;
    assert_eq!(stopped.state.and_then(|state| state.running), Some(false));
    for id in [fork.dind_id(), fork.runner_id()] {
        let inspected = docker.inspect_container(id, None).await?;
        assert_eq!(inspected.state.and_then(|state| state.running), Some(true));
    }
    let containers = [
        trusted.dind_id().to_owned(),
        trusted.runner_id().to_owned(),
        fork.dind_id().to_owned(),
        fork.runner_id().to_owned(),
    ];
    let networks = [
        scenario.fixture.network.clone(),
        scenario.fork_network.clone(),
    ];
    let environment = scenario.fixture.environment.clone();
    scenario.finish().await?;
    assert_removed(&docker, &containers, &networks).await;
    assert_eq!(labelled(&docker, &environment).await?, (0, 0));
    for id in [other.dind_id(), other.runner_id()] {
        let inspected = docker.inspect_container(id, None).await?;
        assert_eq!(inspected.state.and_then(|state| state.running), Some(true));
    }
    docker.inspect_network(&foreign.network, None).await?;
    velnor_runner_host::local::remove_pair(&docker, &other).await?;
    foreign.finish().await?;
    Ok(())
}

fn removed<T: std::fmt::Debug>(result: Result<T, Error>) -> TestResult<()> {
    match result {
        Err(Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        other => Err(format!("expected removed resource, got {other:?}").into()),
    }
}

struct PartialCleanup<'a> {
    retained: &'a str,
    sibling: &'a str,
    volume: &'a str,
    network: &'a str,
    alias: &'a str,
}

impl PartialCleanup<'_> {
    async fn observe(&self, owned: &Fixture, foreign: &Fixture) -> TestResult<()> {
        let docker = &owned.docker;
        docker.inspect_container(self.retained, None).await?;
        docker.inspect_network(&owned.network, None).await?;
        removed(docker.inspect_container(self.sibling, None).await)?;
        removed(docker.inspect_volume(self.volume).await)?;
        removed(docker.inspect_network(self.network, None).await)?;
        removed(docker.inspect_image(self.alias).await)?;
        if labelled(docker, &foreign.environment).await? != (2, 5) {
            return Err("foreign environment resources changed".into());
        }
        docker.inspect_network(&foreign.network, None).await?;
        docker.inspect_image(foreign.image_tag()).await?;
        Ok(())
    }
}

async fn guarded_alias(owned: &Fixture, unique: &str) -> TestResult<(String, Guard)> {
    let alias = format!("velnor-local-test-guard:{unique}");
    let guard = Guard::new(
        owned.docker.clone(),
        &format!("test-alias-{unique}"),
        &alias,
        &format!("velnor-local-test-alias-{unique}"),
    );
    owned
        .docker
        .tag_image(
            &owned.image,
            Some(TagImageOptions {
                repo: Some("velnor-local-test-guard".to_owned()),
                tag: Some(unique.to_owned()),
            }),
        )
        .await?;
    Ok((alias, guard))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_fixture_cleanup_attempts_independent_resources_after_removal_failure()
-> TestResult<()> {
    let Some(foreign) = Fixture::new().await? else {
        return Ok(());
    };
    let foreign_pair = Box::pin(start_pair(&foreign.docker, &foreign.request())).await?;
    for explicit in [true, false] {
        let Some(owned) = Fixture::new().await? else {
            return Err("selected endpoint disappeared".into());
        };
        let pair = Box::pin(start_pair(&owned.docker, &owned.request())).await?;
        let unique = suffix()?;
        let network = format!("velnor-local-test-independent-{unique}");
        owned.guard.track_network(&network);
        owned
            .docker
            .create_network(NetworkCreateRequest {
                name: network.clone(),
                ..Default::default()
            })
            .await?;
        let volume = owned
            .docker
            .create_volume(VolumeCreateRequest {
                labels: Some(std::collections::HashMap::from([(
                    "velnor-local".to_owned(),
                    owned.environment.clone(),
                )])),
                ..Default::default()
            })
            .await?
            .name;
        let (alias, mut alias_guard) = guarded_alias(&owned, &unique).await?;
        let proxy = Proxy::start(&owned.endpoint, Faults::retain_container(pair.dind_id()))?;
        let mut guard = Guard::new(
            proxy.docker.clone(),
            &owned.environment,
            &alias,
            &owned.network,
        );
        guard.track_network(&network);
        let state = PartialCleanup {
            retained: pair.dind_id(),
            sibling: pair.runner_id(),
            volume: &volume,
            network: &network,
            alias: &alias,
        };
        let outcome = if explicit {
            let result = guard.finish().await;
            let observed = state.observe(&owned, &foreign).await;
            let injected = result.err().is_some_and(|error| {
                matches!(error.downcast_ref::<Error>(), Some(Error::DockerResponseServerError {
                    status_code: 500, message,
                }) if message == "injected")
            });
            drop(guard);
            observed.and_then(|()| {
                injected
                    .then_some(())
                    .ok_or_else(|| "finish lost the injected error".into())
            })
        } else {
            drop(guard);
            state.observe(&owned, &foreign).await
        };
        owned.finish().await?;
        alias_guard.finish().await?;
        outcome?;
    }
    for id in [foreign_pair.dind_id(), foreign_pair.runner_id()] {
        foreign.docker.inspect_container(id, None).await?;
    }
    foreign.finish().await?;
    Ok(())
}
