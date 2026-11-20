// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CRASHED, Crash, NEIGHBOR, Prepared, TestResult, bootstrap_seed, db, prepare_version};
use crate::launch::bootstrap::retire_unbound_pair_at;

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_claim_without_a_docker_post_releases_after_its_deadline_dind_create()
-> TestResult<()> {
    Box::pin(never_dispatched(Crash::DindCreate, false)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_claim_without_a_docker_post_releases_after_its_deadline_volume_create()
-> TestResult<()> {
    Box::pin(never_dispatched(Crash::VolumeCreate, false)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_claim_without_a_docker_post_releases_after_its_deadline_network_create()
-> TestResult<()> {
    Box::pin(never_dispatched(Crash::NetworkCreate, false)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_clock_missing_after_commit_uses_a_durable_recovery_deadline() -> TestResult<()> {
    Box::pin(never_dispatched(Crash::DindCreate, true)).await
}

async fn never_dispatched(crash: Crash, unanchored: bool) -> TestResult<()> {
    let case = prepare_version(crash, true)
        .await
        .map_err(|e| format!("prepare {crash:?}: {e}"))?;
    if unanchored {
        crate::launch::bootstrap::unbound_pair_fixtures::seed::unguarded(
            &case.journal,
            &[
                "UPDATE bootstrap_create_claim_clocks SET claimed_at=NULL WHERE reservation_id=2"
                    .to_owned(),
            ],
        )
        .await?;
    }
    let case = case
        .restart()
        .await
        .map_err(|e| format!("first restart {crash:?}: {e}"))?;
    match crash {
        Crash::DindCreate => case.fixture.remove_container(&CRASHED.container_id("dind")),
        Crash::VolumeCreate => case.fixture.remove_volume(&CRASHED.volumes()[3].0),
        Crash::NetworkCreate => case.fixture.remove_network(CRASHED),
        _ => return Err("create crash required".into()),
    }
    let neighbor = bootstrap_seed::state(&case.journal, NEIGHBOR.reservation).await?;
    let objects = case.fixture.objects(NEIGHBOR);
    let before = case.fixture.objects(CRASHED);
    assert!(
        !retire_unbound_pair_at(
            &case.docker,
            &case.scope,
            &case.engine,
            CRASHED.reservation,
            0
        )
        .await?
    );
    let claimed = claim_time(&case).await?;
    assert!(
        !retire_unbound_pair_at(
            &case.docker,
            &case.scope,
            &case.engine,
            CRASHED.reservation,
            claimed + 59
        )
        .await
        .map_err(|e| format!("before deadline {crash:?}: {e}"))?
    );
    assert!(bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    assert_eq!(case.fixture.objects(CRASHED), before);
    let case = case
        .restart()
        .await
        .map_err(|e| format!("second restart {crash:?}: {e}"))?;
    assert_eq!(claim_time(&case).await?, claimed);
    assert!(
        retire_unbound_pair_at(
            &case.docker,
            &case.scope,
            &case.engine,
            CRASHED.reservation,
            claimed + 60
        )
        .await
        .map_err(|e| format!("deadline {crash:?}: {e}"))?,
        "{crash:?} must release a create claim whose POST never happened"
    );
    db::released(&case, &neighbor, &objects).await?;
    let case = case.restart().await?;
    db::released(&case, &neighbor, &objects).await?;
    if matches!(crash, Crash::DindCreate) {
        Box::pin(late_container_is_quarantined(case)).await?;
    }
    Ok(())
}

async fn claim_time(case: &Prepared) -> TestResult<i64> {
    db::number(
        &case.journal,
        "SELECT COALESCE(MIN(claimed_at),0) FROM bootstrap_create_claim_clocks WHERE reservation_id=2",
    )
    .await
}

async fn late_container_is_quarantined(case: Prepared) -> TestResult<()> {
    let publication = case.fixture.delay_create(
        "containers",
        &format!("w{}-dind", CRASHED.nonce()),
        &CRASHED.container_id("dind"),
    );
    publication.release();
    super::late::Family::Container.submit(&case).await?;
    publication.published().await;
    let case = case.restart().await?;
    let mut controller = super::super::controller::Controller::new(&case.scope, &case.engine)?;
    controller
        .turn(&case.journal, &case.docker, &case.engine)
        .await?;
    assert!(
        case.scope
            .claim_bootstrap_start(CRASHED.reservation)
            .await?
            .is_none()
    );
    assert!(
        case.scope
            .claim_bootstrap_container_create(
                CRASHED.reservation,
                crate::launch::bootstrap::BootstrapContainerRole::Dind
            )
            .await?
            .is_none()
    );
    let observed = case
        .docker
        .inspect_container(&CRASHED.container_id("dind"), None)
        .await?;
    assert_eq!(observed.state.and_then(|state| state.running), Some(false));
    assert!(!bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    assert_eq!(db::number(&case.journal,"SELECT COUNT(*) FROM bootstrap_create_absences WHERE reservation_id=2 AND resource_kind='container'").await?,1);
    Ok(())
}
