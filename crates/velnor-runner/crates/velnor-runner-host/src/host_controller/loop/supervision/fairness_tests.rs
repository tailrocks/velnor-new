use std::time::Duration;

use crate::{
    config::RepositoryConnection,
    error::HostError,
    journal::{
        Journal, LocalWorkerExitKind, PhysicalRunnerJitState, PhysicalRunnerOwnership,
        request_lifecycle::tests::{helpers, provision_burst_ready_candidate},
    },
    launch::bootstrap::CleanupRuntimeFixture,
};

use super::tests::{RunningInspectServer, all_candidates, selected_engine};
use super::{LocalWorkerHealthSupervisor, supervise_local_worker_health};

#[test]
fn empty_scopes_and_active_retiring_candidates_resume_after_the_bounded_inspect_pass()
-> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "worker-health-bounded-scope-fairness",
        || {
            Box::pin(async {
                Box::pin(empty_scopes_and_active_retiring_candidates_resume_after_the_bounded_inspect_pass_inner())
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn empty_scopes_and_active_retiring_candidates_resume_after_the_bounded_inspect_pass_inner()
-> Result<(), HostError> {
    let (fixture, binding) = ready_candidate_fixture().await?;
    let empty_bindings = empty_scopes_before_candidate(&fixture.journal).await?;
    assert_empty_scopes_precede_candidate(&binding, &empty_bindings)?;
    let info_docker = fixture.server.docker();
    let selected = selected_engine(&fixture.scope, &info_docker).await?;
    let candidates = genuine_candidates(&fixture.scope).await?;
    let info = info_docker.info().await.map_err(|_| HostError::Docker)?;
    let (inspect_docker, _server) =
        RunningInspectServer::open_first_slow_then_exited(Duration::from_secs(5), &info)?;
    resume_across_empty_scopes(
        &fixture,
        &binding,
        &empty_bindings,
        &selected,
        &inspect_docker,
        &candidates,
    )
    .await?;
    Ok(())
}

fn assert_empty_scopes_precede_candidate(
    binding: &RepositoryConnection,
    empty_bindings: &[RepositoryConnection],
) -> Result<(), HostError> {
    let candidate_identity = binding
        .canonical_identity_bytes()
        .map_err(|_| HostError::Journal)?;
    for empty in empty_bindings {
        assert!(
            empty
                .canonical_identity_bytes()
                .map_err(|_| HostError::Journal)?
                < candidate_identity
        );
    }
    Ok(())
}

async fn genuine_candidates(
    scope: &crate::journal::ConnectionJournal,
) -> Result<Vec<crate::journal::LocalWorkerHealthCandidate>, HostError> {
    let candidates = all_candidates(scope).await?;
    let reservations = scope.physical_runner_reservations().await?;
    assert_eq!(candidates.len(), 5);
    assert_eq!(
        reservations
            .iter()
            .filter(|reservation| {
                reservation.jit_state == PhysicalRunnerJitState::Ready
                    && matches!(&reservation.ownership, PhysicalRunnerOwnership::Known(_))
            })
            .count(),
        4
    );
    assert_eq!(
        reservations
            .iter()
            .filter(|reservation| {
                reservation.jit_state == PhysicalRunnerJitState::Started
                    && matches!(&reservation.ownership, PhysicalRunnerOwnership::Known(_))
            })
            .count(),
        1
    );
    Ok(candidates)
}

async fn resume_across_empty_scopes(
    fixture: &CleanupRuntimeFixture,
    binding: &RepositoryConnection,
    empty_bindings: &[RepositoryConnection],
    selected: &crate::launch::SelectedEngineContext,
    inspect_docker: &bollard::Docker,
    candidates: &[crate::journal::LocalWorkerHealthCandidate],
) -> Result<(), HostError> {
    let first_reservation = candidates[0].reservation_id();
    let next_reservation = candidates[1].reservation_id();
    let mut supervisor = LocalWorkerHealthSupervisor::default();
    supervise_local_worker_health(
        &mut supervisor,
        &fixture.journal,
        inspect_docker,
        selected,
        empty_bindings,
        std::slice::from_ref(binding),
    )
    .await;
    assert_eq!(
        fixture.scope.unresolved_local_worker_exits().await?,
        Vec::new()
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 5);
    supervisor
        .supervise(
            &fixture.journal,
            inspect_docker,
            selected,
            std::slice::from_ref(binding),
            empty_bindings,
        )
        .await;
    assert_eq!(
        fixture.scope.unresolved_local_worker_exits().await?,
        Vec::new()
    );
    supervisor
        .supervise(
            &fixture.journal,
            inspect_docker,
            selected,
            empty_bindings,
            std::slice::from_ref(binding),
        )
        .await;
    let exits = fixture.scope.unresolved_local_worker_exits().await?;
    assert!(exits.iter().any(|exit| {
        exit.reservation_id() == next_reservation
            && exit.kind() == LocalWorkerExitKind::Exited { exit_code: 23 }
    }));
    assert!(
        !exits
            .iter()
            .any(|exit| exit.reservation_id() == first_reservation)
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 5);
    Ok(())
}

async fn empty_scopes_before_candidate(
    journal: &Journal,
) -> Result<Vec<RepositoryConnection>, HostError> {
    let mut bindings = Vec::new();
    for index in 0..5 {
        let binding = helpers::binding(&format!("a-worker-health-empty-{index}"), 700 + index)?;
        journal.connection_scope(&binding).await?;
        bindings.push(binding);
    }
    Ok(bindings)
}

async fn ready_candidate_fixture()
-> Result<(CleanupRuntimeFixture, crate::config::RepositoryConnection), HostError> {
    let fixture =
        crate::journal::request_lifecycle::tests::native_started_intake_burst_fixture().await?;
    fixture.record_started().await?;
    let binding = helpers::binding("native-started-intake", 101)?;
    for (index, nonce) in ['b', 'c', 'd', 'e'].into_iter().enumerate() {
        let request_id = 20_000 + i64::try_from(index).map_err(|_| HostError::Journal)?;
        let _candidate = provision_burst_ready_candidate(
            &fixture.scope,
            &binding,
            request_id,
            nonce,
            800 + i64::try_from(index).map_err(|_| HostError::Journal)?,
            fixture.runner.runner_scale_set_id,
        )
        .await?;
    }
    let fixture = crate::launch::bootstrap::cleanup_runtime_fixture_for_base(fixture).await?;
    Ok((fixture, binding))
}
