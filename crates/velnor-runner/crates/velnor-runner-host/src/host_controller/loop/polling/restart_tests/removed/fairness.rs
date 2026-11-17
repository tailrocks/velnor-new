// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::query_parameters::{RemoveContainerOptionsBuilder, StopContainerOptionsBuilder};

use super::{Controller, RETIRED, Restarted, TestResult, restarted_with_removed_pairs, seed};
use crate::config::RepositoryConnection;
use crate::controller_ipc::ControlState;
use crate::host_controller::loop_driver::{
    connection_poll::{ConnectionPollContext, fairness::FairnessTurn, poll_active_connections},
    setup::configure_shared_capacity,
    state::PreparedConnection,
};
use crate::launch::bootstrap::unbound_pair_fixtures::seed::Pair;

const FIRST: Pair = Pair {
    reservation: 2,
    bound: false,
};
const SECOND: Pair = Pair {
    reservation: 4,
    bound: false,
};
const BUDGET: Duration = Duration::from_secs(5);
const GUARD: Duration = Duration::from_secs(15);
const CHARGE: &str = "SELECT state || '/' || cpu_millicores || '/' || memory_bytes FROM capacity_reservations WHERE id = ?1";
const CHARGED: &str = "uncertain/3000/6442450944";
const GENERATION: &str = "SELECT CAST(claim_generation AS TEXT) FROM capacity_reservation_unbound_retirement_claims WHERE reservation_id = ?1";
const EXPIRED: &str = "SELECT CAST(lease_until > 0 AND lease_until < unixepoch() AS TEXT) FROM capacity_reservation_unbound_retirement_claims WHERE reservation_id = ?1";
const INITIALIZER: &str = "SELECT abort_state || '/' || CAST(abort_lease_until AS TEXT) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";

#[tokio::test(flavor = "current_thread")]
async fn a_persistently_slow_assigned_pair_does_not_starve_a_healthy_assigned_tail()
-> TestResult<()> {
    exercise(Tail::Assigned, false).await
}

#[tokio::test(flavor = "current_thread")]
async fn a_persistently_slow_assigned_pair_does_not_starve_an_initializer_tail() -> TestResult<()> {
    exercise(Tail::Initializer, false).await
}

#[tokio::test(flavor = "current_thread")]
async fn an_initializer_before_a_slow_assigned_pair_finishes_in_the_first_pass() -> TestResult<()> {
    exercise(Tail::Initializer, true).await
}

#[tokio::test(flavor = "current_thread")]
async fn a_persistently_slow_assigned_pair_does_not_starve_an_unknown_assignment_tail()
-> TestResult<()> {
    exercise(Tail::Unknown, false).await
}

#[derive(Clone, Copy)]
enum Tail {
    Assigned,
    Initializer,
    Unknown,
}

async fn exercise(kind: Tail, initializer_first: bool) -> TestResult<()> {
    let (restarted, controller, slow, healthy) = prepare(kind, initializer_first).await?;
    configure_shared_capacity(&restarted.journal, &controller.config).await?;
    let Controller {
        config,
        images,
        image_authority,
        state,
        prepared,
    } = controller;
    let control_state = ControlState::new(&config);
    let context = ConnectionPollContext {
        config: &config,
        journal: &restarted.journal,
        docker: &restarted.docker,
        selected_engine: &restarted.engine,
        worker_images: &images,
        state: state.path(),
        image_authority: &image_authority,
        admission_ready: false,
        control_state: &control_state,
        retirement_budget: BUDGET,
    };
    let pending = vec![prepared.binding.clone()];
    let mut connections = vec![prepared];
    let slow_objects = restarted.fixture.objects(slow);
    restarted
        .fixture
        .gate_container_inspection(&slow.container_id("runner"));
    reconcile(&restarted, &context, &pending, &mut connections).await?;
    assert_eq!(restarted.fixture.inspection_gate_hits(), 1);
    assert_eq!(restarted.fixture.objects(slow), slow_objects);
    assert_slow_charged(&restarted, slow).await?;
    if initializer_first {
        assert_released(&restarted, healthy, kind).await?;
    } else {
        assert_eq!(
            seed::text(&restarted.journal, CHARGE, healthy.reservation).await?,
            CHARGED
        );
    }
    expire_only_slow_lease(&restarted, slow).await?;
    reconcile(&restarted, &context, &pending, &mut connections).await?;
    assert_eq!(restarted.fixture.inspection_gate_hits(), 2);
    assert_eq!(restarted.fixture.objects(slow), slow_objects);
    assert_released(&restarted, healthy, kind).await?;
    assert_slow_charged(&restarted, slow).await
}

async fn prepare(
    kind: Tail,
    initializer_first: bool,
) -> TestResult<(Restarted, Controller, Pair, Pair)> {
    let (restarted, controller) = restarted_with_removed_pairs(&[FIRST, SECOND]).await?;
    let (slow, healthy) = if initializer_first {
        (SECOND, FIRST)
    } else {
        (FIRST, SECOND)
    };
    let scope = &controller.prepared.scope;
    seed::assigned_pair(&restarted.journal, scope, slow).await?;
    match kind {
        Tail::Assigned => seed::assigned_pair(&restarted.journal, scope, healthy).await?,
        Tail::Initializer => prepare_initializer(&restarted, &controller, healthy).await?,
        Tail::Unknown => {
            let reservation = scope
                .physical_runner_reservations()
                .await?
                .into_iter()
                .find(|reservation| reservation.reservation_id == healthy.reservation)
                .ok_or("unknown reservation")?;
            assert!(reservation.ownership.assignment().is_none());
            assert!(
                scope
                    .uncertain_unbound_pairs()
                    .await?
                    .contains(&healthy.reservation)
            );
        }
    }
    Ok((restarted, controller, slow, healthy))
}

async fn reconcile(
    restarted: &Restarted,
    context: &ConnectionPollContext<'_>,
    pending: &[RepositoryConnection],
    connections: &mut Vec<PreparedConnection>,
) -> TestResult<()> {
    let recovery = poll_active_connections(
        ConnectionPollContext { ..*context },
        pending,
        connections,
        FairnessTurn::new(true, false),
    );
    tokio::pin!(recovery);
    tokio::time::timeout(GUARD, async {
        tokio::select! {
            result = &mut recovery => {
                result?;
                Err::<(), Box<dyn std::error::Error>>("removed connection poll returned before its assigned pair stalled".into())
            }
            () = restarted.fixture.hung() => Ok(()),
        }
    })
    .await??;
    tokio::time::pause();
    tokio::time::advance(BUDGET + Duration::from_secs(1)).await;
    tokio::time::resume();
    tokio::time::timeout(GUARD, &mut recovery).await??;
    Ok(())
}

async fn expire_only_slow_lease(restarted: &Restarted, slow: Pair) -> TestResult<()> {
    let generation = seed::text(&restarted.journal, GENERATION, slow.reservation).await?;
    seed::unguarded(&restarted.journal, &[format!(
        "UPDATE capacity_reservation_unbound_retirement_claims SET lease_until = unixepoch() - 1 WHERE reservation_id = {}",
        slow.reservation
    )]).await?;
    assert_eq!(
        seed::text(&restarted.journal, EXPIRED, slow.reservation).await?,
        "1"
    );
    assert_eq!(
        seed::text(&restarted.journal, GENERATION, slow.reservation).await?,
        generation
    );
    assert_slow_charged(restarted, slow).await
}

async fn assert_slow_charged(restarted: &Restarted, slow: Pair) -> TestResult<()> {
    assert_eq!(
        seed::text(&restarted.journal, CHARGE, slow.reservation).await?,
        CHARGED
    );
    assert!(
        restarted
            .fixture
            .has_container(&slow.container_id("runner"))
    );
    Ok(())
}

async fn assert_released(restarted: &Restarted, pair: Pair, kind: Tail) -> TestResult<()> {
    assert_eq!(
        seed::text(&restarted.journal, seed::RESERVATION, pair.reservation).await?,
        RETIRED
    );
    assert_eq!(restarted.fixture.objects(pair), Vec::<String>::new());
    if let Tail::Initializer = kind {
        assert_eq!(
            seed::text(&restarted.journal, INITIALIZER, pair.reservation).await?,
            "absent/0"
        );
    }
    Ok(())
}

pub(super) async fn prepare_initializer(
    restarted: &Restarted,
    controller: &Controller,
    pair: Pair,
) -> TestResult<()> {
    seed::initializer(&restarted.journal, pair, "not_requested", "not_requested").await?;
    seed::unguarded(&restarted.journal, &[
        format!("DELETE FROM capacity_reservation_bootstrap_containers WHERE reservation_id = {}", pair.reservation),
        format!("UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'not_requested', runner_container_id = NULL, dind_container_id = NULL WHERE reservation_id = {}", pair.reservation),
        format!("UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'response_received', create_state = 'uncertain' WHERE reservation_id = {}", pair.reservation),
    ]).await?;
    for role in ["runner", "dind"] {
        let id = pair.container_id(role);
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
    let scope = &controller.prepared.scope;
    let reservation = scope
        .physical_runner_reservations()
        .await?
        .into_iter()
        .find(|reservation| reservation.reservation_id == pair.reservation)
        .ok_or("initializer reservation")?;
    assert!(reservation.ownership.assignment().is_none());
    assert!(
        scope
            .storage_initializer_abort_candidate(pair.reservation)
            .await?
            .is_some()
    );
    assert!(!restarted.fixture.has_initializer(pair));
    assert_eq!(restarted.fixture.objects(pair).len(), 6);
    Ok(())
}
