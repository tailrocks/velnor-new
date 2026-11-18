// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::query_parameters::{RemoveContainerOptionsBuilder, StopContainerOptionsBuilder};

use super::{
    Controller, FAILED, Restarted, TestResult, UNCERTAIN, assert_retired, freeze,
    restarted_with_a_removed_connection, seed, stalled,
};
use crate::host_controller::loop_driver::polling::{PositivePollContext, pre_jit};

const BUDGET: Duration = Duration::from_secs(2);
const INITIALIZER: &str = "SELECT abort_state || '/' || (abort_lease_until > 0) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";
const LEASE: &str = "SELECT CAST(abort_lease_until AS TEXT) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";

#[tokio::test(flavor = "current_thread")]
async fn a_removed_connection_deadline_releases_its_initializer_abort_lease() -> TestResult<()> {
    exercise(false).await
}

#[tokio::test(flavor = "current_thread")]
async fn a_startup_deadline_releases_its_initializer_abort_lease() -> TestResult<()> {
    exercise(true).await
}

async fn interrupted_initializer() -> TestResult<(Restarted, Controller)> {
    let (restarted, controller) = restarted_with_a_removed_connection().await?;
    seed::initializer(&restarted.journal, FAILED, "not_requested", "not_requested").await?;
    seed::unguarded(
        &restarted.journal,
        &[
            format!(
                "DELETE FROM capacity_reservation_bootstrap_containers WHERE reservation_id = {}",
                FAILED.reservation
            ),
            format!(
                "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'not_requested', runner_container_id = NULL, dind_container_id = NULL WHERE reservation_id = {}",
                FAILED.reservation
            ),
            format!(
                "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'response_received', create_state = 'uncertain' WHERE reservation_id = {}",
                FAILED.reservation
            ),
        ],
    )
    .await?;
    for role in ["runner", "dind"] {
        let id = FAILED.container_id(role);
        restarted
            .docker
            .stop_container(
                &id,
                Some(StopContainerOptionsBuilder::default().t(0).build()),
            )
            .await?;
        restarted
            .docker
            .remove_container(
                &id,
                Some(
                    RemoveContainerOptionsBuilder::default()
                        .force(false)
                        .v(false)
                        .build(),
                ),
            )
            .await?;
    }
    let reservation = controller
        .prepared
        .scope
        .physical_runner_reservations()
        .await?
        .into_iter()
        .find(|reservation| reservation.reservation_id == FAILED.reservation)
        .ok_or("initializer reservation")?;
    assert!(reservation.ownership.assignment().is_none());
    assert_retryable(&controller).await?;
    assert_eq!(restarted.fixture.objects(FAILED).len(), 6);
    Ok((restarted, controller))
}

async fn reconcile(
    restarted: &Restarted,
    controller: &Controller,
    startup: bool,
) -> TestResult<()> {
    if startup {
        controller
            .clean_up_at_startup(
                &restarted.journal,
                &restarted.docker,
                &restarted.engine,
                BUDGET,
            )
            .await;
    } else {
        pre_jit::reconcile(
            &controller.prepared.scope,
            0,
            &PositivePollContext {
                config: &controller.config,
                journal: &restarted.journal,
                docker: &restarted.docker,
                selected_engine: &restarted.engine,
                worker_images: &controller.images,
                state: controller.state.path(),
                image_authority: &controller.image_authority,
                admission_ready: false,
                retirement_budget: BUDGET,
            },
        )
        .await?;
    }
    Ok(())
}

async fn exercise(startup: bool) -> TestResult<()> {
    let (restarted, controller) = interrupted_initializer().await?;
    restarted
        .fixture
        .hang_after(format!("GET /volumes/w{}-metadata", FAILED.nonce()));
    let clock = freeze();
    let recovery = reconcile(&restarted, &controller, startup);
    tokio::pin!(recovery);
    stalled(&restarted.fixture, &mut recovery).await?;
    assert_eq!(
        seed::text(&restarted.journal, INITIALIZER, FAILED.reservation).await?,
        "requested/1"
    );
    clock.advance(BUDGET + Duration::from_secs(1)).await;
    recovery.await?;
    assert_eq!(
        seed::text(&restarted.journal, seed::RESERVATION, FAILED.reservation).await?,
        UNCERTAIN
    );
    assert_eq!(restarted.fixture.objects(FAILED).len(), 6);
    assert_eq!(
        seed::text(&restarted.journal, INITIALIZER, FAILED.reservation).await?,
        "uncertain/0"
    );
    assert_eq!(
        seed::text(&restarted.journal, LEASE, FAILED.reservation).await?,
        "0"
    );
    assert_retryable(&controller).await?;
    reconcile(&restarted, &controller, startup).await?;
    assert_retired(&restarted).await?;
    reconcile(&restarted, &controller, startup).await?;
    assert_retired(&restarted).await?;
    drop(clock);
    Ok(())
}

async fn assert_retryable(controller: &Controller) -> TestResult<()> {
    assert!(
        controller
            .prepared
            .scope
            .storage_initializer_abort_candidate(FAILED.reservation)
            .await?
            .is_some()
    );
    Ok(())
}
