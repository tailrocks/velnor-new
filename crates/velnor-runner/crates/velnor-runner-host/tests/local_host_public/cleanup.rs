// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use bollard::errors::Error;
use velnor_runner_host::local::start_pair;

use super::scenario::{Scenario, finish_job};
use super::support::{Fixture, TestResult, labelled};

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
    let scenario = Scenario::new().await?;
    let foreign = Fixture::new().await?;
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
