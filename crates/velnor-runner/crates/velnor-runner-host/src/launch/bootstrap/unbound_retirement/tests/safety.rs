// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::{Journal, UnboundRetirementPhase};

use super::fixture::FixtureDocker;
use super::seed::{self, RESERVATION};
use super::{FAILED, RETIRED, TestResult, abandon_and_retire_unbound_pair, retire_unbound_pair};

const CHARGED: &str = "uncertain/00/uncertain/0/1/none/";
const CHARGED_CONTAINERS_GONE: &str = "uncertain/00/uncertain/0/1/none/containers_absent";

#[tokio::test(flavor = "multi_thread")]
async fn a_container_with_a_wrong_label_is_left_alone_and_the_reservation_stays_charged()
-> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    let before = fixture.objects(FAILED);
    fixture.relabel_container(&FAILED.container_id("runner"));

    let outcome =
        abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await;

    assert!(outcome.is_err());
    assert_eq!(fixture.objects(FAILED), before);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        CHARGED
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_foreign_container_under_the_pairs_name_is_left_alone_and_the_reservation_stays_charged()
-> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    let foreign = format!("f{:063x}", FAILED.reservation);
    fixture.reissue_container(&FAILED.container_id("runner"), &foreign);

    let outcome =
        abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await;

    assert!(outcome.is_err());
    assert!(fixture.has_container(&foreign));
    assert!(fixture.has_container(&FAILED.container_id("dind")));
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        CHARGED
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_container_with_the_pairs_id_but_another_name_is_left_alone_and_the_reservation_stays_charged()
-> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    let before = fixture.objects(FAILED);
    fixture.rename_container(&FAILED.container_id("runner"), "someone-elses-container");

    let outcome =
        abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await;

    assert!(outcome.is_err());
    assert_eq!(fixture.objects(FAILED), before);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        CHARGED
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_volume_with_a_wrong_label_is_left_alone_and_the_reservation_stays_charged()
-> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    let volumes = FAILED.volumes().map(|(name, _)| name);
    fixture.relabel_volume(&volumes[0]);

    let outcome =
        abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await;

    assert!(outcome.is_err());
    assert!(fixture.objects(FAILED).contains(&volumes[0]));
    assert!(fixture.objects(FAILED).contains(&FAILED.network_id()));
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        CHARGED_CONTAINERS_GONE
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn volume_progress_cannot_be_recorded_before_container_progress() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(FAILED.reservation).await?;
    let claim = scope
        .claim_unbound_retirement(FAILED.reservation)
        .await?
        .ok_or("claim")?;
    assert_eq!(claim.engine_id(), engine.engine_id());
    let digest = "c".repeat(64);

    let early = scope
        .record_unbound_retirement_phase(&claim, UnboundRetirementPhase::VolumesAbsent, &digest)
        .await;

    assert!(early.is_err());
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        CHARGED
    );
    scope
        .record_unbound_retirement_phase(&claim, UnboundRetirementPhase::ContainersAbsent, &digest)
        .await?;
    scope
        .record_unbound_retirement_phase(&claim, UnboundRetirementPhase::VolumesAbsent, &digest)
        .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_after_the_volumes_are_removed_converges_on_restart() -> TestResult<()> {
    let volumes = FAILED.volumes().map(|(name, _)| name);
    let last = volumes.last().cloned().ok_or("volume")?;
    converges_after_crash_following(&format!("DELETE /volumes/{last}"), CHARGED_CONTAINERS_GONE)
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_after_the_network_is_removed_converges_on_restart() -> TestResult<()> {
    converges_after_crash_following(
        &format!("DELETE /networks/{}", FAILED.network_id()),
        "uncertain/00/uncertain/0/1/none/containers_absent,volumes_absent",
    )
    .await
}

async fn converges_after_crash_following(request: &str, interrupted: &str) -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;

    fixture.hang_after(request.to_owned());
    tokio::select! {
        _ = Box::pin(abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation)) => {
            return Err("the retirement ended before the daemon was killed".into());
        }
        () = fixture.hung() => {}
    }
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        interrupted
    );

    journal.recover_after_restart_without_authority().await?;
    assert!(retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    assert_eq!(fixture.objects(FAILED), Vec::<String>::new());

    journal.recover_after_restart_without_authority().await?;
    assert!(!retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_release_changes_nothing_and_the_retry_releases_once() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    seed::execute(
        &journal,
        "CREATE TRIGGER test_block_release BEFORE UPDATE ON capacity_reservations WHEN NEW.state = 'released' BEGIN SELECT RAISE(ABORT, 'blocked'); END",
    )
    .await?;

    let failed =
        abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await;

    assert!(failed.is_err());
    assert_eq!(fixture.objects(FAILED), Vec::<String>::new());
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        "uncertain/00/uncertain/0/1/none/containers_absent,volumes_absent"
    );

    seed::execute(&journal, "DROP TRIGGER test_block_release").await?;
    journal.recover_after_restart_without_authority().await?;
    assert!(retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    assert!(!retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    Ok(())
}
