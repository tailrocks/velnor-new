// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::models::{ContainerCreateBody, NetworkCreateRequest};
use bollard::query_parameters::CreateContainerOptions;

use super::super::controller::Controller;
use super::{CRASHED, Crash, NEIGHBOR, Prepared, TestResult, bootstrap_seed, db, prepare};

#[derive(Clone, Copy)]
pub(super) enum Family {
    Container,
    Network,
}

impl Family {
    fn crash(self) -> Crash {
        match self {
            Self::Container => Crash::DindCreate,
            Self::Network => Crash::NetworkCreate,
        }
    }
    fn kind(self) -> &'static str {
        match self {
            Self::Container => "containers",
            Self::Network => "networks",
        }
    }
    fn role(self) -> &'static str {
        match self {
            Self::Container => "dind",
            Self::Network => "private",
        }
    }
    fn id(self) -> String {
        match self {
            Self::Container => CRASHED.container_id("dind"),
            Self::Network => CRASHED.network_id(),
        }
    }
    fn name(self) -> String {
        match self {
            Self::Container => format!("w{}-dind", CRASHED.nonce()),
            Self::Network => format!("velnor-v{}-private", CRASHED.nonce()),
        }
    }
    fn remove(self, case: &Prepared) {
        match self {
            Self::Container => case.fixture.remove_container(&self.id()),
            Self::Network => case.fixture.remove_network(CRASHED),
        }
    }
    pub(super) async fn submit(self, case: &Prepared) -> TestResult<()> {
        match self {
            Self::Container => {
                let labels = HashMap::from([
                    ("io.velnor.managed".to_owned(), "true".to_owned()),
                    ("io.velnor.reservation-id".to_owned(), "2".to_owned()),
                    ("io.velnor.operation-id".to_owned(), "1".to_owned()),
                    (
                        "io.velnor.connection-id".to_owned(),
                        "conn-test-1".to_owned(),
                    ),
                    ("io.velnor.role".to_owned(), "dind".to_owned()),
                ]);
                case.docker
                    .create_container(
                        Some(CreateContainerOptions {
                            name: Some(self.name()),
                            ..Default::default()
                        }),
                        ContainerCreateBody {
                            image: Some("image-test".to_owned()),
                            labels: Some(labels),
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            Self::Network => {
                let connection = case.journal.connection().await?;
                let mut rows = connection.query("SELECT labels_json FROM capacity_reservation_networks WHERE reservation_id=2", ()).await?;
                let labels = serde_json::from_str(
                    &rows
                        .next()
                        .await?
                        .ok_or("network claim")?
                        .get::<String>(0)?,
                )?;
                case.docker
                    .create_network(NetworkCreateRequest {
                        name: self.name(),
                        labels: Some(labels),
                        driver: Some("bridge".to_owned()),
                        ..Default::default()
                    })
                    .await?;
            }
        }
        Ok(())
    }
    async fn target(self, case: &Prepared) -> TestResult<()> {
        let kind = match self {
            Self::Container => "container",
            Self::Network => "network",
        };
        db::target(&case.journal, kind, self.role(), &self.name(), &self.id()).await
    }
}

async fn delayed_publication(family: Family) -> TestResult<()> {
    let case = prepare(family.crash(), false).await?;
    family.remove(&case);
    let publication = case
        .fixture
        .delay_create(family.kind(), &family.name(), &family.id());
    let mut original = Box::pin(family.submit(&case));
    let submission = tokio::select! {
        result = &mut original => return Err(format!("original create completed before acceptance barrier: {result:?}").into()),
        submission = publication.accepted() => submission,
    };
    assert_eq!(submission.name, family.name());
    assert!(submission.labels.is_object());
    drop(original);
    let case = case.restart().await?;
    let neighbor = bootstrap_seed::state(&case.journal, NEIGHBOR.reservation).await?;
    let objects = case.fixture.objects(NEIGHBOR);
    case.fixture.take_requests();
    let mut controller = Controller::new(&case.scope, &case.engine)?;
    controller
        .turn(&case.journal, &case.docker, &case.engine)
        .await?;
    let first = db::unresolved(&case.journal, matches!(family, Family::Network)).await?;
    assert!(
        case.fixture.has_initializer(CRASHED),
        "unresolved creation must keep the ready-held helper under its cleanup owner"
    );
    let requests = case.fixture.take_requests();
    let inspect = match family {
        Family::Container => format!("GET /containers/{}/json", family.name()),
        Family::Network => format!("GET /networks/{}", family.name()),
    };
    assert!(
        requests.contains(&inspect),
        "uncertain create must actually be inspected before deferral: {requests:?}"
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("/create") || request.ends_with("/start"))
    );
    publication.release();
    publication.published().await;
    assert!(case.fixture.objects(CRASHED).contains(&family.id()));
    delete_and_complete(&case, &mut controller, family, first).await?;
    db::released(&case, &neighbor, &objects).await
}

async fn delete_and_complete(
    case: &Prepared,
    controller: &mut Controller,
    family: Family,
    previous: i64,
) -> TestResult<()> {
    let deletion = case.fixture.delay_delete(family.kind(), &family.id());
    let mut cleanup = Box::pin(controller.turn(&case.journal, &case.docker, &case.engine));
    tokio::select! {
        result = &mut cleanup => return Err(format!("cleanup completed before original target DELETE: {result:?}").into()),
        () = deletion.reached() => {},
    }
    assert!(
        db::generation(&case.journal).await? > previous,
        "next poll must acquire a strictly newer claim"
    );
    family.target(case).await?;
    deletion.release();
    cleanup.await?;
    Ok(())
}

async fn crash_after_delete(family: Family) -> TestResult<()> {
    let case = prepare(family.crash(), false).await?.restart().await?;
    let neighbor = bootstrap_seed::state(&case.journal, NEIGHBOR.reservation).await?;
    let objects = case.fixture.objects(NEIGHBOR);
    case.fixture.take_requests();
    let deletion = case.fixture.delay_delete(family.kind(), &family.id());
    let mut controller = Controller::new(&case.scope, &case.engine)?;
    let mut cleanup = Box::pin(controller.turn(&case.journal, &case.docker, &case.engine));
    tokio::select! {
        result = &mut cleanup => return Err(format!("cleanup completed before target DELETE crash barrier: {result:?}").into()),
        () = deletion.reached() => {},
    }
    family.target(&case).await?;
    assert!(!case.fixture.objects(CRASHED).contains(&family.id()));
    drop(cleanup);
    drop(controller);
    deletion.release();
    let case = case.restart().await?;
    family.target(&case).await?;
    let mut controller = Controller::new(&case.scope, &case.engine)?;
    controller
        .turn(&case.journal, &case.docker, &case.engine)
        .await?;
    db::released(&case, &neighbor, &objects).await
}

#[tokio::test(flavor = "multi_thread")]
async fn original_container_post_publishes_after_restart_404_and_client_drop() -> TestResult<()> {
    Box::pin(delayed_publication(Family::Container)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn original_network_post_publishes_after_restart_404_and_client_drop() -> TestResult<()> {
    Box::pin(delayed_publication(Family::Network)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn exact_container_target_survives_a_crash_after_delete_effect() -> TestResult<()> {
    Box::pin(crash_after_delete(Family::Container)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn exact_network_target_survives_a_crash_after_delete_effect() -> TestResult<()> {
    Box::pin(crash_after_delete(Family::Network)).await
}
