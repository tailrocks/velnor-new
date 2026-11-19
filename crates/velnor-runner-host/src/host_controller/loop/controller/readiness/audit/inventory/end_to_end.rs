// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "end_to_end/engine.rs"]
mod engine;
#[path = "end_to_end/refresh.rs"]
mod refresh;
#[path = "end_to_end/runtime.rs"]
mod runtime;
#[path = "end_to_end/seed.rs"]
mod seed;

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::super::RefreshDependencies;
use super::super::RemoteAuditFuture;
use super::{SettlingAges, audit_inventory_with_remote};
use crate::config::HostConfig;
use crate::error::HostError;
use crate::journal::Journal;
use seed::{DIND_IMAGE, RESERVATION, RUNNER_IMAGE, Seeded};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const OTHER_IMAGE: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";

struct World {
    dir: tempfile::TempDir,
    journal: Journal,
    docker: bollard::Docker,
    config: HostConfig,
    seeded: Seeded,
}

async fn world(containers: BTreeMap<String, Value>) -> TestResult<World> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    engine::serve(&socket, containers)?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let config = HostConfig::parse(seed::CONFIG)?;
    let seeded = seed::started_pair(&journal, &docker, &config).await?;
    Ok(World {
        dir,
        journal,
        docker,
        config,
        seeded,
    })
}

async fn readiness(world: &World) -> Result<Option<String>, HostError> {
    let remote_present = |_, _, _| -> RemoteAuditFuture { Box::pin(async { Ok(true) }) };
    audit_inventory_with_remote(
        &world.docker,
        &world.journal,
        &world.config,
        &world.seeded.selected,
        std::slice::from_ref(&world.seeded.poll),
        &RefreshDependencies {
            remote_audit: &remote_present,
            settling: &SettlingAges::default(),
        },
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_with_matching_image_kinds_publishes_readiness() -> TestResult<()> {
    let world = world(engine::healthy()?).await?;
    let published = readiness(&world).await?;
    assert!(published.is_some_and(|digest| digest.len() == 64));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_whose_container_lacks_the_image_kind_publishes_no_readiness() -> TestResult<()> {
    for (runner, dind) in [(None, Some("dind")), (Some("runner"), None)] {
        let containers = engine::containers((RUNNER_IMAGE, runner), (DIND_IMAGE, dind))?;
        let world = world(containers).await?;
        assert_eq!(readiness(&world).await, Err(HostError::Ownership));
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_carrying_the_other_roles_image_kind_publishes_no_readiness() -> TestResult<()> {
    let containers =
        engine::containers((RUNNER_IMAGE, Some("dind")), (DIND_IMAGE, Some("runner")))?;
    let world = world(containers).await?;
    assert_eq!(readiness(&world).await, Err(HostError::Ownership));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_running_an_unadmitted_image_publishes_no_readiness() -> TestResult<()> {
    let containers = engine::containers((OTHER_IMAGE, Some("runner")), (DIND_IMAGE, Some("dind")))?;
    let world = world(containers).await?;
    assert_eq!(readiness(&world).await, Err(HostError::Ownership));
    Ok(())
}

async fn identity_after(statement: &str) -> TestResult<Result<(), HostError>> {
    let world = world(engine::healthy()?).await?;
    seed::unguarded(&world.journal, &[statement.to_owned()]).await?;
    Ok(world
        .seeded
        .scope
        .storage_cleanup_identity(RESERVATION)
        .await
        .map(|_| ()))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admitted_image_ids_are_read_back_for_the_matching_pair() -> TestResult<()> {
    let world = world(engine::healthy()?).await?;
    let identity = world
        .seeded
        .scope
        .storage_cleanup_identity(RESERVATION)
        .await?
        .ok_or("identity")?;
    assert_eq!(identity.runner_image_id(), RUNNER_IMAGE);
    assert_eq!(identity.dind_image_id(), DIND_IMAGE);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_root_backing_row_for_another_container_is_refused() -> TestResult<()> {
    for role in ["runner-root", "dind-root"] {
        let statement = format!(
            "UPDATE capacity_reservation_root_backing_mappings SET container_id = '{}' WHERE reservation_id = {RESERVATION} AND role = '{role}'",
            "f".repeat(64)
        );
        assert_eq!(
            identity_after(&statement).await?,
            Err(HostError::Journal),
            "{role}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_root_backing_role_is_refused() -> TestResult<()> {
    for role in ["runner-root", "dind-root"] {
        let statement = format!(
            "DELETE FROM capacity_reservation_root_backing_mappings WHERE reservation_id = {RESERVATION} AND role = '{role}'"
        );
        assert_eq!(
            identity_after(&statement).await?,
            Err(HostError::Journal),
            "{role}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_journal_refuses_to_store_a_duplicated_role_or_an_empty_image_id() -> TestResult<()> {
    let world = world(engine::healthy()?).await?;
    for statement in [
        format!(
            "UPDATE capacity_reservation_root_backing_mappings SET role = 'runner-root' WHERE reservation_id = {RESERVATION} AND role = 'dind-root'"
        ),
        format!(
            "UPDATE capacity_reservation_root_backing_mappings SET image_runtime_id = '' WHERE reservation_id = {RESERVATION} AND role = 'dind-root'"
        ),
    ] {
        assert!(seed::unguarded(&world.journal, &[statement]).await.is_err());
    }
    Ok(())
}
