// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "tests/deadline.rs"]
mod deadline;
#[path = "tests/eligibility.rs"]
mod eligibility;
#[path = "tests/fixture.rs"]
pub(crate) mod fixture;
#[path = "tests/initializer.rs"]
mod initializer;
#[path = "tests/migration.rs"]
mod migration;
#[path = "tests/renewal.rs"]
mod renewal;
#[path = "tests/safety.rs"]
mod safety;
#[path = "tests/seed.rs"]
pub(crate) mod seed;
#[path = "tests/shape.rs"]
mod shape;

use crate::journal::Journal;

use super::{abandon_and_retire_unbound_pair, retire_unbound_pair};
use fixture::FixtureDocker;
use seed::{Pair, RESERVATION};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

pub(crate) const FAILED: Pair = Pair {
    reservation: 2,
    bound: false,
};
const BOUND: Pair = Pair {
    reservation: 4,
    bound: true,
};
const FRESH: Pair = Pair {
    reservation: 6,
    bound: false,
};
pub(crate) const RETIRED: &str =
    "released/11/uncertain/1/0/unbound_retired/containers_absent,volumes_absent";

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_whose_helper_check_failed_is_removed_and_its_reservation_released() -> TestResult<()>
{
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED, BOUND])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED, BOUND]).await?;
    fixture.take_requests();

    assert!(!retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(fixture.take_requests(), Vec::<String>::new());
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        "reserved/00/pending/0/1/none/"
    );

    assert!(abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    assert_eq!(fixture.objects(FAILED), Vec::<String>::new());

    fixture.take_requests();
    assert!(!abandon_and_retire_unbound_pair(&docker, &scope, &engine, BOUND.reservation).await?);
    assert_eq!(fixture.take_requests(), Vec::<String>::new());
    assert_eq!(fixture.objects(BOUND).len(), 8);
    assert_eq!(
        seed::text(&journal, RESERVATION, BOUND.reservation).await?,
        "reserved/00/pending/0/1/none/"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_after_the_containers_are_removed_converges_on_restart() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;

    fixture.hang_after(format!(
        "DELETE /containers/{}",
        FAILED.container_id("dind")
    ));
    tokio::select! {
        _ = Box::pin(abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation)) => {
            return Err("the retirement ended before the daemon was killed".into());
        }
        () = fixture.hung() => {}
    }
    let mut volumes_and_network = FAILED.volumes().map(|(name, _)| name).to_vec();
    volumes_and_network.push(FAILED.network_id());
    assert_eq!(fixture.objects(FAILED), volumes_and_network);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        "uncertain/00/uncertain/0/1/none/"
    );

    fixture.take_requests();
    assert!(!retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(fixture.take_requests(), Vec::<String>::new());

    journal.recover_after_restart_without_authority().await?;
    assert!(retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    assert_eq!(fixture.objects(FAILED), Vec::<String>::new());
    assert!(
        !fixture
            .take_requests()
            .iter()
            .any(|request| request.starts_with("DELETE /containers/"))
    );

    journal.recover_after_restart_without_authority().await?;
    assert!(!retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(fixture.take_requests(), Vec::<String>::new());
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retirement_claim_refuses_resource_binding_while_a_normal_reservation_binds()
-> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, &[FRESH, FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FRESH, FAILED]).await?;

    seed::bind_resources(&journal, FRESH, engine.docker_root_digest()).await?;
    assert_eq!(seed::resources_bound(&journal, FRESH.reservation).await?, 1);

    assert!(abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    let error = seed::bind_resources(&journal, FAILED, engine.docker_root_digest())
        .await
        .err()
        .ok_or("binding a claimed reservation succeeded")?;
    assert!(
        error
            .to_string()
            .contains("A retirement claim prevents this operation.")
    );
    assert_eq!(
        seed::resources_bound(&journal, FAILED.reservation).await?,
        0
    );
    Ok(())
}
