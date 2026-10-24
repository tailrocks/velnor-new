use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::journal::MetadataSettlementResult;

struct CensusFixture {
    failure: crate::launch::RunnerCreateFailure,
    scope: crate::journal::ConnectionJournal,
    journal: crate::journal::Journal,
    _scratch: crate::journal::request_lifecycle::tests::helpers::Scratch,
    state_dir: PathBuf,
    images: crate::launch::VerifiedWorkerImages,
    selected: crate::launch::SelectedEngineContext,
    config: crate::config::HostConfig,
    prepared: crate::host_controller::loop_driver::state::PreparedConnection,
    admissions: AtomicUsize,
    request: crate::journal::AssignmentIdentity,
}

#[test]
fn settled_conflict_discards_cached_population_until_a_fresh_sample() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "jit-conflict-settlement-fresh-population",
        || Box::pin(settled_conflict_fresh_population_case()),
    )
}

async fn settled_conflict_fresh_population_case() -> Result<(), String> {
    let mut fixture = CensusFixture::new().await?;
    Box::pin(fixture.missing_sample_cannot_plan_successor()).await?;
    fixture.fresh_sample_enables_plan_only().await?;
    Box::pin(fixture.finish_without_second_runner_create()).await
}

impl CensusFixture {
    async fn new() -> Result<Self, String> {
        let failure = crate::launch::runner_create_409_with_jobs(1)
            .await
            .map_err(|error| format!("runner create fixture: {error}"))?;
        failure.set_probe_available_bytes(32 * 1024 * 1024 * 1024);
        let images = failure.images().clone();
        let selected = failure.engine().clone();
        let scope = failure.scope().clone();
        let journal = failure.host_journal().clone();
        release_failed_attempt(&failure, &scope, &journal)
            .await
            .map_err(|error| format!("release failed attempt: {error}"))?;

        let scratch = crate::journal::request_lifecycle::tests::helpers::Scratch::new()
            .map_err(|error| stage_error("scratch", error))?;
        let state = scratch.file();
        let state_dir = state
            .parent()
            .ok_or_else(|| "The test state path has no parent.".to_owned())?
            .to_path_buf();
        let binding = failure.binding().clone();
        let mut acquired = scope
            .acquired_authorized_requests(64)
            .await
            .map_err(|error| format!("read acquired requests: {error}"))?;
        if acquired.len() != 1 || acquired[0].request_id() != Some(41) {
            return Err("The fixture must retain exactly its acquired request.".to_owned());
        }
        let request = acquired.remove(0);
        let config = crate::config::HostConfig::test_with_job_authority_policy(&binding)
            .map_err(|error| stage_error("test host config", error))?;
        let mut prepared = crate::host_controller::loop_driver::polling::assigned_population::missing_statistics::prepared_connection(
            &scope,
            &binding,
            &images,
            &state_dir,
        )
        .await
        .map_err(|error| stage_error("prepared connection", error))?;
        prepared.assigned_total = Some(1);
        assert!(!super::settlement_clears_population(
            &mut prepared,
            MetadataSettlementResult::HeldAtCapacity,
        ));
        assert_eq!(prepared.assigned_total, Some(1));
        assert!(super::settlement_clears_population(
            &mut prepared,
            MetadataSettlementResult::Settled,
        ));
        assert_eq!(prepared.assigned_total, None);

        Ok(Self {
            request,
            failure,
            scope,
            journal,
            _scratch: scratch,
            state_dir,
            images,
            selected,
            config,
            prepared,
            admissions: AtomicUsize::new(0),
        })
    }

    async fn missing_sample_cannot_plan_successor(&mut self) -> Result<(), String> {
        let missing = poll_statistics(self, None, 92_002)
            .await
            .map_err(|error| format!("missing statistics poll: {error}"))?;
        self.prepared.record_assigned_total(missing.assigned_total);
        assert_eq!(self.prepared.assigned_total, None);
        assert_eq!(self.admissions.load(Ordering::Relaxed), 0);

        let secret = Arc::new(crate::launch::ReleaseReadSecret::test_token());
        let provider =
            crate::launch::IntakeCompositionProvider::new(&self.selected, self.images.source_sha());
        let context = crate::host_controller::loop_driver::polling::PositivePollContext {
            config: &self.config,
            journal: &self.journal,
            docker: self.failure.docker(),
            selected_engine: &self.selected,
            worker_images: &self.images,
            state: &self.state_dir,
            release_read_secret: &secret,
            admission_ready: true,
            start_permit_provider: Some(&provider),
        };
        super::super::reconcile(&mut self.prepared, &context)
            .await
            .map_err(|error| stage_error("reconcile without statistics", error))?;
        assert_no_successor(&self.scope, self.failure.assignment())
            .await
            .map_err(|error| format!("assert no successor: {error}"))
    }

    async fn fresh_sample_enables_plan_only(&mut self) -> Result<(), String> {
        let fresh = poll_statistics(self, Some(1), 92_003)
            .await
            .map_err(|error| format!("fresh statistics poll: {error}"))?;
        self.prepared.record_assigned_total(fresh.assigned_total);
        assert_eq!(self.prepared.assigned_total, Some(1));
        let policy = self
            .config
            .validated_job_authority_policy(&self.prepared.binding.connection_id)
            .map_err(|error| stage_error("validate job authority policy", error))?
            .ok_or_else(|| "The test source policy is absent.".to_owned())?;
        let plan = super::super::build_plan(&self.prepared, 1, policy)
            .await
            .map_err(|error| stage_error("build successor plan", error))?;
        assert_eq!(
            plan.actions,
            vec![
                crate::host_controller::AssignedPopulationAction::ReserveSuccessor {
                    request: self.request.clone()
                }
            ]
        );
        assert_eq!(self.admissions.load(Ordering::Relaxed), 0);
        Ok(())
    }

    async fn finish_without_second_runner_create(self) -> Result<(), String> {
        let captured = self
            .failure
            .finish()
            .await
            .map_err(|error| format!("finish runner fixture: {error}"))?;
        if let Some(error) = captured.server_error {
            return Err(format!("The Docker fixture failed: {error}."));
        }
        let runner_creates = captured
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
            .collect::<Vec<_>>();
        assert_eq!(runner_creates.len(), 1);
        assert_eq!(runner_creates[0].response_status, Some(409));
        Ok(())
    }
}

async fn poll_statistics(
    fixture: &mut CensusFixture,
    assigned_total: Option<i64>,
    message_id: i64,
) -> Result<crate::launch::PositivePollOutcome, String> {
    crate::host_controller::loop_driver::polling::assigned_population::missing_statistics::poll_statistics(
        &fixture.scope,
        &mut fixture.prepared,
        &fixture.admissions,
        assigned_total,
        message_id,
    )
    .await
}

async fn release_failed_attempt(
    failure: &crate::launch::RunnerCreateFailure,
    scope: &crate::journal::ConnectionJournal,
    journal: &crate::journal::Journal,
) -> Result<(), String> {
    let provider = crate::launch::IntakeCompositionProvider::new(
        failure.engine(),
        failure.images().source_sha(),
    );
    crate::launch::bootstrap::remove_bootstrap_after_terminal_failure(
        failure.docker(),
        scope,
        journal,
        failure.engine(),
        failure.images(),
        failure.reservation_id(),
        &provider,
    )
    .await
    .map_err(show_error)?;
    journal
        .configure_capacity_with_operational_limits(
            crate::journal::CapacityLimits {
                jobs: 1,
                cpu_millicores: 4_000,
                memory_bytes: 32 * 1024 * 1024 * 1024,
            },
            crate::journal::OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 256 * 1024 * 1024,
            },
        )
        .await
        .map_err(show_error)
}

async fn assert_no_successor(
    scope: &crate::journal::ConnectionJournal,
    original: &crate::journal::AssignmentIdentity,
) -> Result<(), String> {
    let original = scope
        .reservation(original)
        .await
        .map_err(show_error)?
        .ok_or_else(|| "The original reservation is missing after settlement.".to_owned())?;
    if original.state != crate::journal::ReservationState::Released {
        return Err(format!(
            "Expected original reservation to be released after settlement, found {:?}.",
            original.state
        ));
    }
    let reservations = scope
        .physical_runner_reservations()
        .await
        .map_err(show_error)?;
    if !reservations.is_empty() {
        return Err(format!(
            "Expected no active reservation after missing statistics, found {}.",
            reservations.len()
        ));
    }
    Ok(())
}

fn show_error(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

fn stage_error(stage: &str, error: impl std::fmt::Debug) -> String {
    format!("{stage}: {error:?}")
}
