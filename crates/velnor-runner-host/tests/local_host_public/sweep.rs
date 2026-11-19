// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::models::ContainerCreateBody;
use bollard::query_parameters::RemoveContainerOptions;

use super::scenario::Scenario;
use super::support::{TestResult, labelled};
use super::{position, runner};

async fn orphan(scenario: &Scenario, environment: &str) -> TestResult<String> {
    let labels = HashMap::from([
        ("velnor-local".to_owned(), environment.to_owned()),
        (
            "velnor-local.pair".to_owned(),
            format!("{environment}-pair"),
        ),
        ("velnor-local.side".to_owned(), "trusted".to_owned()),
        (
            "velnor-local.agent".to_owned(),
            format!("{environment}-agent"),
        ),
    ]);
    labelled_container(scenario, labels).await
}

async fn service(scenario: &Scenario, environment: &str) -> TestResult<String> {
    let labels = HashMap::from([("velnor-local".to_owned(), environment.to_owned())]);
    labelled_container(scenario, labels).await
}

async fn labelled_container(
    scenario: &Scenario,
    labels: HashMap<String, String>,
) -> TestResult<String> {
    let created = scenario
        .fixture
        .docker
        .create_container(
            None::<bollard::query_parameters::CreateContainerOptions>,
            ContainerCreateBody {
                image: Some(scenario.fixture.image.clone()),
                labels: Some(labels),
                ..Default::default()
            },
        )
        .await?;
    Ok(created.id)
}

async fn exists(scenario: &Scenario, id: &str) -> bool {
    scenario
        .fixture
        .docker
        .inspect_container(id, None)
        .await
        .is_ok()
}

async fn discard(scenario: &Scenario, id: &str) {
    let _gone = scenario
        .fixture
        .docker
        .remove_container(
            id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;
}

#[tokio::test]
async fn local_start_sweeps_the_leftovers_of_its_environment_only() -> TestResult<()> {
    let Some(mut scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let environment = scenario.fixture.environment.clone();
    let other = format!("{environment}-other");
    let leftover = orphan(&scenario, &environment).await?;
    let neighbour = orphan(&scenario, &other).await?;
    let services = service(&scenario, &environment).await?;
    let plan = scenario.plan(1, 0)?;
    scenario.start(plan);
    let run = async {
        runner(&scenario, "trusted", &[]).await?;
        TestResult::Ok(!exists(&scenario, &leftover).await)
    }
    .await;
    let stopped = scenario.stop().await?;
    let events = scenario.fake().events();
    let kept = exists(&scenario, &neighbour).await;
    let service_kept = exists(&scenario, &services).await;
    discard(&scenario, &neighbour).await;
    discard(&scenario, &services).await;
    discard(&scenario, &leftover).await;
    scenario.finish().await?;
    assert!(run?);
    let agent = format!("delete:trusted:{environment}-agent");
    assert!(position(&events, &agent).is_some(), "{events:?}");
    assert!(position(&events, &format!("delete:trusted:{other}")).is_none());
    assert_eq!(stopped, Ok(()));
    assert!(kept);
    assert!(
        service_kept,
        "the sweep removed a container that is not a pair"
    );
    Ok(())
}

#[tokio::test]
async fn local_exit_sweeps_what_appeared_after_the_start_and_spares_other_environments()
-> TestResult<()> {
    let Some(mut scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let docker = scenario.fixture.docker.clone();
    let environment = scenario.fixture.environment.clone();
    let other = format!("{environment}-other");
    let plan = scenario.plan(1, 0)?;
    scenario.start(plan);
    let run = async {
        runner(&scenario, "trusted", &[]).await?;
        let late = orphan(&scenario, &environment).await?;
        let neighbour = orphan(&scenario, &other).await?;
        TestResult::Ok((late, neighbour))
    }
    .await;
    let stopped = scenario.stop().await?;
    let events = scenario.fake().events();
    let left = labelled(&docker, &environment).await;
    let mut survivors = None;
    if let Ok((late, neighbour)) = &run {
        survivors = Some((
            exists(&scenario, late).await,
            exists(&scenario, neighbour).await,
        ));
        discard(&scenario, late).await;
        discard(&scenario, neighbour).await;
    }
    scenario.finish().await?;
    run?;
    let agent = format!("delete:trusted:{environment}-agent");
    assert!(position(&events, &agent).is_some(), "{events:?}");
    assert!(position(&events, &format!("delete:trusted:{other}")).is_none());
    assert_eq!(stopped, Ok(()));
    assert_eq!(left?, (0, 0));
    assert_eq!(survivors, Some((false, true)));
    Ok(())
}
