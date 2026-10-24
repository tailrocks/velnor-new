use super::super::helpers::{
    record_observed_storage_cleanup, record_owned_network_cleanup, settle_storage_metadata,
};
use super::fixtures::{self, LinkedLifecycle};
use crate::error::HostError;
use crate::journal::{AssignmentKey, ReservationState};
use crate::listen::Secret;
use velnor_runner_github::TransportFail;
use velnor_runner_github::session::{Exchange, Method, SessionRequest, Transport};

#[tokio::test]
async fn completed_available_remains_owned_after_observed_cleanup_and_reopen()
-> Result<(), HostError> {
    let fixture = fixtures::prepare_linked_lifecycle(100, 117, 'c').await?;
    let actual = fixture
        .scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: fixture.actual_request_id,
        })
        .await?;
    super::record_completed(&fixture, &actual).await?;
    assert_available_is_owned(&fixture).await?;
    assert_changed_job_and_source_are_rejected(&fixture).await?;

    record_runner_absence(&fixture).await?;
    fixtures::check_cleanup_and_settlement(&fixture).await?;
    let fixture = fixtures::reopen(fixture).await?;
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture.physical)
            .await?
            .map(|reservation| reservation.state),
        Some(ReservationState::Released)
    );
    assert!(
        fixture
            .scope
            .intent(fixture.intent_id)
            .await?
            .cleanup_proven
    );
    assert_available_is_owned(&fixture).await?;
    assert_changed_job_and_source_are_rejected(&fixture).await
}

#[tokio::test]
async fn stale_completion_lease_cannot_retire_current_physical_attempt() -> Result<(), HostError> {
    let fixture = fixtures::prepare_linked_lifecycle(100, 117, 'c').await?;
    let actual = fixture
        .scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: fixture.actual_request_id,
        })
        .await?;
    super::record_completed(&fixture, &actual).await?;
    record_runner_absence(&fixture).await?;
    record_observed_storage_cleanup(&fixture.scope, fixture.reservation_id).await?;
    fixture
        .scope
        .record_remote_cleanup(fixture.reservation_id)
        .await?;
    fixture
        .scope
        .record_local_cleanup(fixture.reservation_id)
        .await?;
    record_owned_network_cleanup(&fixture.scope, fixture.reservation_id).await?;
    crate::journal::connection::storage::read_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            let changed = connection
                .execute(
                    "UPDATE connection_completion_cleanup SET lease_until = 0 WHERE intent_id = ?1",
                    [fixture.intent_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            Ok(())
        },
    )
    .await?;
    assert!(
        settle_storage_metadata(&fixture.scope, &fixture.provisioned, 't')
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture.physical)
            .await?
            .map(|reservation| reservation.state),
        Some(ReservationState::Reserved)
    );
    assert!(
        fixture
            .scope
            .current_physical_attempt(fixture.reservation_id)
            .await?
            .is_some()
    );
    Ok(())
}

pub(super) async fn record_runner_absence(fixture: &LinkedLifecycle) -> Result<(), HostError> {
    let mut launches = fixture.scope.due_completed_launches(0, 1).await?;
    let launch = launches.pop().ok_or(HostError::Journal)?;
    if launch.intent.id != fixture.intent_id {
        return Err(HostError::Journal);
    }
    let claim = fixture
        .scope
        .claim_completion_cleanup(fixture.intent_id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let mut transport = EmptyRunnerTransport::default();
    let admin = Secret::new("fixture token");
    crate::launch::reconcile_for_test(&fixture.scope, &mut transport, &admin, &launch, claim)
        .await?;
    assert_eq!(transport.requests.len(), 2);
    let expected_query = format!("agentName={}&api-version=6.0-preview", fixture.runner.name);
    assert!(transport.requests.iter().all(|(is_get, path, query)| {
        *is_get && path == "_apis/distributedtask/pools/0/agents" && query == &expected_query
    }));
    Ok(())
}

#[derive(Default)]
struct EmptyRunnerTransport {
    requests: Vec<(bool, String, String)>,
}

impl Transport for EmptyRunnerTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push((
            request.method == Method::Get,
            request.path.clone(),
            request.query.clone().unwrap_or_default(),
        ));
        Ok(Exchange {
            status: 200,
            body: br#"{"count":0,"value":[]}"#.to_vec(),
        })
    }
}

async fn assert_available_is_owned(fixture: &LinkedLifecycle) -> Result<(), HostError> {
    assert!(
        fixture
            .scope
            .owns_terminal_available_requests(
                501,
                &[(
                    fixture.actual_request_id,
                    fixture.source.clone(),
                    Some(fixture.job_id.clone()),
                )],
            )
            .await?
    );
    Ok(())
}

async fn assert_changed_job_and_source_are_rejected(
    fixture: &LinkedLifecycle,
) -> Result<(), HostError> {
    assert!(
        !fixture
            .scope
            .owns_terminal_available_requests(
                501,
                &[(
                    fixture.actual_request_id,
                    fixture.source.clone(),
                    Some("different-job".to_owned()),
                )],
            )
            .await?
    );
    let mut changed_source = fixture.source.clone();
    changed_source.workflow_run_id = changed_source.workflow_run_id.map(|run| run + 1);
    assert!(
        !fixture
            .scope
            .owns_terminal_available_requests(
                501,
                &[(
                    fixture.actual_request_id,
                    changed_source,
                    Some(fixture.job_id.clone()),
                )],
            )
            .await?
    );
    assert!(
        !fixture
            .scope
            .owns_terminal_available_requests(
                501,
                &[(
                    fixture.actual_request_id + 1,
                    fixture.source.clone(),
                    Some(fixture.job_id.clone()),
                )],
            )
            .await?
    );
    Ok(())
}
