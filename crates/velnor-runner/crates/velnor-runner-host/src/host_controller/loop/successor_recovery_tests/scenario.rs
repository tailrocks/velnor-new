use super::super::{
    polling::{PositivePollContext, reconcile_assigned_population},
    state::{PreparedConnection, SessionCredentials},
};
use super::controller::{self, ControllerState};

struct FailedAttempt {
    failure: crate::launch::RunnerCreateFailure,
    binding: crate::config::RepositoryConnection,
    scope: crate::journal::ConnectionJournal,
    journal: crate::journal::Journal,
    request: crate::journal::AssignmentIdentity,
    old_intent_id: i64,
    acquire_state: Option<crate::journal::RequestAcquireState>,
}

pub(super) async fn max_jobs_one_successor() -> Result<(), String> {
    let attempt = FailedAttempt::start().await?;
    attempt.release_failed_attempt().await?;
    attempt.recover_controller_demand().await?;
    attempt.finish_and_check().await
}

impl FailedAttempt {
    async fn start() -> Result<Self, String> {
        let failure = Box::pin(crate::launch::runner_create_409_with_jobs(1)).await?;
        failure.set_probe_available_bytes(128 * 1024 * 1024 * 1024);
        let binding = failure.binding().clone();
        let scope = failure.scope().clone();
        let journal = failure.host_journal().clone();
        let request = acquired_request(&scope, failure.reservation_id()).await?;
        let old_intent_id = intent_for(&scope, failure.reservation_id()).await?;
        let acquire_state = scope
            .request_acquire_state(&request)
            .await
            .map_err(|error| error.to_string())?;
        Ok(Self {
            failure,
            binding,
            scope,
            journal,
            request,
            old_intent_id,
            acquire_state,
        })
    }

    async fn release_failed_attempt(&self) -> Result<(), String> {
        let provider = crate::launch::IntakeCompositionProvider::new(
            self.failure.engine(),
            self.failure.images().source_sha(),
        );
        crate::launch::bootstrap::remove_bootstrap_after_terminal_failure(
            self.failure.docker(),
            &self.scope,
            &self.journal,
            self.failure.engine(),
            self.failure.images(),
            self.failure.reservation_id(),
            &provider,
        )
        .await
        .map_err(|error| format!("The original pre-JIT attempt did not clean up: {error}."))?;
        assert_eq!(
            self.scope
                .reservation(self.failure.assignment())
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "The original physical reservation is absent.".to_owned())?
                .state,
            crate::journal::ReservationState::Released,
        );
        assert_eq!(
            self.journal
                .occupied_reservation_count()
                .await
                .map_err(|error| error.to_string())?,
            1,
            "The acquired logical demand must remain charged until a successor exists."
        );
        Ok(())
    }

    async fn recover_controller_demand(&self) -> Result<(), String> {
        let state = ControllerState::new(&self.binding)?;
        let config = controller::controller_config(&self.binding)?;
        let (mut prepared, mut link, mut session) = controller::prepared_connection(
            &self.failure,
            &self.journal,
            state.path(),
            &self.binding,
        )
        .await?;
        poll_fresh_assigned_demand(
            &self.scope,
            &mut prepared,
            &mut link,
            &mut session,
            &self.request,
            &config,
        )
        .await?;
        prepared.credentials = Some(SessionCredentials {
            link,
            session,
            github_pat: crate::listen::Secret::new("test-token"),
        });
        self.reconcile_successor(&mut prepared, &config, &state)
            .await
    }

    async fn reconcile_successor(
        &self,
        prepared: &mut PreparedConnection,
        config: &crate::config::HostConfig,
        state: &ControllerState,
    ) -> Result<(), String> {
        let secret = crate::launch::ReleaseReadSecret::test_token();
        let provider = crate::launch::IntakeCompositionProvider::new(
            self.failure.engine(),
            self.failure.images().source_sha(),
        );
        let context = PositivePollContext {
            config,
            journal: &self.journal,
            docker: self.failure.docker(),
            selected_engine: self.failure.engine(),
            worker_images: self.failure.images(),
            state: state.path(),
            release_read_secret: &secret,
            admission_ready: true,
            start_permit_provider: Some(&provider),
        };
        reconcile_assigned_population(prepared, &context)
            .await
            .map_err(|error| {
                format!("The controller could not recover the acquired demand: {error}.")
            })
    }

    async fn finish_and_check(self) -> Result<(), String> {
        assert_successor_rows(
            &self.scope,
            &self.request,
            self.acquire_state,
            self.failure.reservation_id(),
            self.old_intent_id,
        )
        .await?;
        let captured = self.failure.finish().await?;
        if captured.server_error.is_some() {
            return Err(format!(
                "The Docker fixture failed: {:?}.",
                captured.server_error
            ));
        }
        let creates: Vec<_> = captured
            .requests
            .iter()
            .filter(|request| {
                request.method == "POST"
                    && request
                        .target
                        .split('?')
                        .next()
                        .is_some_and(|path| path.ends_with("/containers/create"))
                    && serde_json::from_slice::<serde_json::Value>(&request.body)
                        .is_ok_and(|body| body["Labels"]["io.velnor.role"] == "runner")
            })
            .collect();
        assert_eq!(
            creates.len(),
            2,
            "Exactly two physical Runner creates are expected."
        );
        assert_eq!(creates[0].response_status, Some(409));
        assert_eq!(creates[1].response_status, Some(201));
        assert_eq!(captured.jit_input, Vec::<u8>::new());
        Ok(())
    }
}

async fn poll_fresh_assigned_demand(
    scope: &crate::journal::ConnectionJournal,
    prepared: &mut PreparedConnection,
    link: &mut crate::listen::Link,
    session: &mut velnor_runner_github::QueueSession,
    request: &crate::journal::AssignmentIdentity,
    config: &crate::config::HostConfig,
) -> Result<(), String> {
    let poll = controller::assigned_statistics(1);
    let response = crate::launch::test_positive_poll_once_with_response(
        link,
        prepared.set_id,
        session,
        scope,
        prepared.session_intent,
        &controller::header_allocation(scope, prepared.session_intent).await?,
        0,
        poll,
        "https://queue.example.invalid/messages".to_owned(),
        Some("queue-token".to_owned()),
        async |_link, _session, _path, _queue, _polled| Ok(()),
        async |_link, _poll, _origin, _path, _queue, _admin| {
            Err(crate::scale_set::EnsureError::AdmissionClosed)
        },
    )
    .await
    .map_err(|error| format!("The fresh positive poll failed: {error}."))?;
    prepared.record_assigned_total(response.assigned_total);
    ensure_demand_is_successor_ready(scope, prepared, request, config, response.assigned_total)
        .await
}

async fn ensure_demand_is_successor_ready(
    scope: &crate::journal::ConnectionJournal,
    prepared: &PreparedConnection,
    request: &crate::journal::AssignmentIdentity,
    config: &crate::config::HostConfig,
    assigned_total: Option<i64>,
) -> Result<(), String> {
    if prepared.assigned_total != Some(1) || assigned_total != Some(1) {
        return Err("The controller did not retain fresh TotalAssignedJobs=1.".to_owned());
    }
    let demand = scope
        .acquired_authorized_requests(64)
        .await
        .map_err(|error| error.to_string())?;
    if !demand.iter().any(|acquired| acquired == request) {
        return Err("The acquired demand was lost.".to_owned());
    }
    let policy = config
        .validated_job_authority_policy(&prepared.binding.connection_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The current source policy is absent.".to_owned())?;
    if !scope
        .acquired_successor_ready(request, policy.generation(), policy.policy_sha256())
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("The proven terminal attempt is not eligible for a successor.".to_owned());
    }
    Ok(())
}

async fn acquired_request(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
) -> Result<crate::journal::AssignmentIdentity, String> {
    scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.reservation_id == reservation_id)
        .and_then(|row| row.acquired_request)
        .ok_or_else(|| "The terminal attempt lost its acquired request.".to_owned())
}

async fn intent_for(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
) -> Result<i64, String> {
    scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.reservation_id == reservation_id)
        .map(|row| row.intent_id)
        .ok_or_else(|| "The initial physical attempt is absent.".to_owned())
}

async fn assert_successor_rows(
    scope: &crate::journal::ConnectionJournal,
    request: &crate::journal::AssignmentIdentity,
    acquire_state: Option<crate::journal::RequestAcquireState>,
    old_reservation_id: i64,
    old_intent_id: i64,
) -> Result<(), String> {
    let current = scope
        .request_acquire_state(request)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(current, acquire_state);
    let rows = scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        rows.iter()
            .all(|row| row.reservation_id != old_reservation_id)
    );
    let successors: Vec<_> = rows
        .iter()
        .filter(|row| {
            row.acquired_request.as_ref() == Some(request)
                && row.reservation_id != old_reservation_id
                && row.reservation.state != crate::journal::ReservationState::Released
        })
        .collect();
    assert_eq!(
        successors.len(),
        1,
        "One fresh physical attempt is required."
    );
    assert_ne!(successors[0].intent_id, old_intent_id);
    assert_eq!(
        rows.len(),
        1,
        "The successor must be the only active reservation."
    );
    Ok(())
}
