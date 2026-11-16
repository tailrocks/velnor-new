// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use super::super::super::super::{RefreshDependencies, refresh_with_dependencies};
use super::super::SettlingAges;
use super::seed;
use super::{OTHER_IMAGE, TestResult, World, engine, world};
use crate::controller_ipc::ControlState;
use crate::error::HostError;
use crate::launch::ReadinessSample;

const OPERATIONAL_CAPACITY: &str = "UPDATE host_capacity_policy SET operational_cpu_millicores = 1000, operational_memory_bytes = 1073741824";

async fn primed(world: &World) -> TestResult<ControlState> {
    std::fs::write(world.dir.path().join("host.toml"), seed::CONFIG)?;
    seed::unguarded(&world.journal, &[OPERATIONAL_CAPACITY.to_owned()]).await?;
    let control = ControlState::new(&world.config);
    control.set_admission_ready(true);
    let binding = world.config.connections().first().ok_or("connection")?;
    let poll = &world.seeded.poll;
    control.record_session_poll_observation(
        binding,
        poll.scale_set_id(),
        poll.session_intent_id(),
        poll.session_id(),
        None,
    )?;
    control.cache_readiness_sample(ReadinessSample::for_test(&world.seeded.selected))?;
    Ok(control)
}

async fn refresh_against(
    world: &World,
    docker: &bollard::Docker,
    control: &ControlState,
) -> TestResult<(Result<(), HostError>, bool)> {
    let remote_present = |_: &_, _: Option<&_>, _: i64| Ok(true);
    let outcome = refresh_with_dependencies(
        world.dir.path(),
        docker,
        &world.journal,
        &world.seeded.selected,
        control,
        None,
        &RefreshDependencies {
            remote_audit: &remote_present,
            settling: &SettlingAges::default(),
        },
    )
    .await;
    let (_, config_sha256) =
        crate::readiness_scan::controller_config(world.dir.path()).ok_or("config")?;
    let published = control.readiness_matches(
        &config_sha256,
        world.seeded.selected.engine_id(),
        world.seeded.selected.docker_root_digest(),
        &world.journal.instance_id().await?,
        world.journal.revision().await?,
        Instant::now(),
    )?;
    Ok((outcome, published))
}

async fn refreshed(world: &World) -> TestResult<(Result<(), HostError>, bool)> {
    refresh_against(world, &world.docker, &primed(world).await?).await
}

#[tokio::test(flavor = "multi_thread")]
async fn refreshing_a_matching_pair_publishes_readiness() -> TestResult<()> {
    let world = world(engine::healthy()?).await?;
    assert_eq!(refreshed(&world).await?, (Ok(()), true));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn refreshing_a_pair_with_a_wrong_image_kind_publishes_no_readiness() -> TestResult<()> {
    let cases = [
        engine::containers((seed::RUNNER_IMAGE, None), (seed::DIND_IMAGE, Some("dind")))?,
        engine::containers(
            (seed::RUNNER_IMAGE, Some("dind")),
            (seed::DIND_IMAGE, Some("runner")),
        )?,
        engine::containers(
            (OTHER_IMAGE, Some("runner")),
            (seed::DIND_IMAGE, Some("dind")),
        )?,
    ];
    for containers in cases {
        let world = world(containers).await?;
        assert_eq!(refreshed(&world).await?, (Err(HostError::Ownership), false));
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_audit_withdraws_readiness_published_by_an_earlier_refresh() -> TestResult<()> {
    let world = world(engine::healthy()?).await?;
    let control = primed(&world).await?;
    assert_eq!(
        refresh_against(&world, &world.docker, &control).await?,
        (Ok(()), true)
    );
    let socket = world.dir.path().join("swapped.sock");
    let swapped = engine::containers((seed::RUNNER_IMAGE, None), (seed::DIND_IMAGE, Some("dind")))?;
    engine::serve(&socket, swapped)?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    assert_eq!(
        refresh_against(&world, &docker, &control).await?,
        (Err(HostError::Ownership), false)
    );
    Ok(())
}
