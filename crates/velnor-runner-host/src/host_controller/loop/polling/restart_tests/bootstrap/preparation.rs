// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CRASHED, Crash, FixtureDocker, Journal, NEIGHBOR, Prepared, TestResult, bootstrap_seed, db,
    external_prefix, seed,
};

pub(super) async fn prepare(crash: Crash, _foreign: bool) -> TestResult<Prepared> {
    prepare_version(crash, false).await
}

pub(super) async fn prepare_version(crash: Crash, current: bool) -> TestResult<Prepared> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[CRASHED, NEIGHBOR])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let path = dir.path().join("launch.db");
    let journal = if current {
        Journal::open(&path).await?
    } else {
        Journal::open_historical_v48_for_test(&path)
            .await
            .map_err(|e| format!("initial opening journal: {e}"))?
    };
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[CRASHED, NEIGHBOR]).await?;
    bootstrap_seed::prepare(&journal, &scope, CRASHED, crash)
        .await
        .map_err(|e| format!("preparing {crash:?}: {e}"))?;
    crate::launch::bootstrap::unbound_pair_fixtures::fixture::bound_neighbor::install(
        &journal, NEIGHBOR, &engine,
    )
    .await?;
    bootstrap_seed::capacity(&journal).await?;
    let policy = bootstrap_seed::authority(&journal).await?;
    bootstrap_seed::bind_neighbor(&scope, NEIGHBOR, &policy).await?;
    if !matches!(crash, Crash::VolumeCreate | Crash::MetadataRecorded) {
        fixture.add_initializer(CRASHED);
    }
    external_prefix(&fixture, crash);
    if matches!(crash, Crash::NetworkRecorded) {
        let name = format!("velnor-v{}-private", CRASHED.nonce());
        let publication = fixture.delay_create("networks", &name, &CRASHED.network_id());
        publication.release();
        crate::launch::bootstrap::create_owned_network(
            &docker,
            &scope,
            &engine,
            CRASHED.reservation,
        )
        .await?;
        publication.published().await;
    }
    if current {
        db::assert_current(&journal).await?;
    } else {
        db::assert_historical(&journal, crash).await?;
    }
    Ok(Prepared {
        directory: dir,
        fixture,
        docker,
        path,
        journal,
        scope,
        engine,
        policy,
    })
}
