//! Check that a separate readiness thread runs during a blocked poll.

use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

#[path = "blocked_poll/transport.rs"]
mod transport;
use transport::{BlockingPoll, RefreshFailure, SampleTiming, refresh_failure};

use crate::{
    config::RepositoryConnection, controller_ipc::ControlState, error::HostError,
    launch::SelectedEngineContext,
};

use super::super::super::worker::spawn_refresh_worker;
use super::super::{
    BusyHostFixture, on_large_test_stack, poll_receipt, prepare_busy_host_without_sample_on_thread,
    refresh_with_dependencies,
};

#[test]
fn independent_worker_samples_while_controller_poll_blocks() -> Result<(), String> {
    on_large_test_stack(independent_worker_samples_while_controller_poll_blocks_async)
}

async fn independent_worker_samples_while_controller_poll_blocks_async() -> Result<(), String> {
    let host = Box::pin(prepare_busy_host_without_sample_on_thread()).await?;
    let receipt = poll_receipt(&host)?;
    let pending = begin_pending_header_poll(&host, &receipt).await?;
    run_blocked_poll(&host, &receipt, pending).await?;
    assert_blocked_poll_cleanup(host).await
}

struct PendingHeaderPoll {
    allocation: crate::journal::HeaderAllocation,
    attempt: crate::journal::HeaderPollAttempt,
    capacity_before: crate::journal::ReadinessCapacityUsage,
}

async fn begin_pending_header_poll(
    host: &BusyHostFixture,
    receipt: &crate::controller_ipc::SessionPollReceipt,
) -> Result<PendingHeaderPoll, String> {
    let allocation = host
        .fixture
        .scope
        .session_header_allocation(receipt.session_intent_id())
        .await
        .map_err(|error| error.to_string())?;
    let attempt = host
        .fixture
        .scope
        .begin_session_poll(receipt.session_intent_id(), &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let capacity_before = host
        .fixture
        .journal
        .readiness_capacity_usage()
        .await
        .map_err(|error| error.to_string())?;
    host.control
        .begin_session_poll(
            &host.binding,
            receipt.scale_set_id(),
            receipt.session_intent_id(),
            receipt.session_id(),
            std::time::Instant::now() + Duration::from_secs(60),
        )
        .map_err(|error| format!("The bound session poll did not start: {error}."))?;
    Ok(PendingHeaderPoll {
        allocation,
        attempt,
        capacity_before,
    })
}

async fn run_blocked_poll(
    host: &BusyHostFixture,
    receipt: &crate::controller_ipc::SessionPollReceipt,
    pending: PendingHeaderPoll,
) -> Result<(), String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (poll_entered_tx, poll_entered_rx) = mpsc::channel();
    let poll_started = Instant::now();
    let worker = start_refresh_worker(host, ready_tx, poll_entered_rx, poll_started)
        .map_err(|error| error.to_string())?;
    let (_, config_digest) = crate::readiness_scan::controller_config(&host.state)
        .ok_or_else(|| "The controller test configuration could not be read.".to_owned())?;
    let journal_instance = host
        .fixture
        .journal
        .instance_id()
        .await
        .map_err(|error| error.to_string())?;
    let mut transport = BlockingPoll {
        ready: ready_rx,
        poll_entered: Some(poll_entered_tx),
        poll_started,
        diagnostic: None,
        control: host.control.clone(),
        config_digest,
        engine_id: host.selected.engine_id().to_owned(),
        docker_root_digest: host.selected.docker_root_digest().to_owned(),
        journal_instance,
        expected_capacity: pending.allocation.target(),
    };
    let total_capacity = pending.allocation.target();
    let poll_result =
        crate::listen::poll_path_with(&mut transport, "queue-token", "messages", total_capacity, 0);
    let response = poll_result.map_err(|error| {
        let docker_requests = host
            .docker
            .observed_request_summary()
            .unwrap_or_else(|trace_error| {
                format!("The Docker request trace is unavailable. Error: {trace_error}.")
            });
        format!(
            "The scoped queue poll failed while the controller poll was blocked after {:?}: {error}. Detail: {}. Docker requests: [{}].",
            transport.poll_started.elapsed(),
            transport
                .diagnostic
                .take()
                .unwrap_or_else(|| "No readiness diagnostic is available.".to_owned()),
            docker_requests
        )
    })?;
    if response != crate::listen::PollPathResult::Response(velnor_runner_github::Poll::Empty) {
        return Err(format!(
            "The scoped queue poll returned an unexpected response: {response:?}."
        ));
    }
    let capacity_before = pending.capacity_before;
    settle_returned_poll(host, receipt, pending).await?;
    let capacity_after = host
        .fixture
        .journal
        .readiness_capacity_usage()
        .await
        .map_err(|error| error.to_string())?;
    if capacity_after != capacity_before || capacity_after.jobs == 0 {
        return Err(format!(
            "The readiness check changed or released the held reservation: before={capacity_before:?}, after={capacity_after:?}."
        ));
    }
    drop(worker);
    host.control
        .invalidate_session_poll(host.binding.connection_id.as_str())
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn settle_returned_poll(
    host: &BusyHostFixture,
    receipt: &crate::controller_ipc::SessionPollReceipt,
    pending: PendingHeaderPoll,
) -> Result<(), String> {
    let proof = crate::launch::CompletedHeaderPoll::after_test_response(
        pending.attempt,
        &pending.allocation,
        &host.fixture.scope,
        receipt.scale_set_id(),
        receipt.session_id(),
    )
    .map_err(|error| format!("The exact returned header poll was rejected: {error}."))?;
    host.fixture
        .scope
        .settle_session_poll(receipt.session_intent_id(), proof)
        .await
        .map_err(|error| format!("The returned header poll did not settle: {error}."))
}

async fn assert_blocked_poll_cleanup(host: BusyHostFixture) -> Result<(), String> {
    let requests = host.docker.finish_observed().await?;
    if requests.len() != 2 + host.identity.volumes().len() + 1 {
        return Err(format!(
            "The audit used an incomplete Docker inventory: {requests:?}."
        ));
    }
    if host
        .fixture
        .journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("The readiness sample left its probe operation active.".to_owned());
    }
    Ok(())
}

fn start_refresh_worker(
    host: &BusyHostFixture,
    signal: mpsc::Sender<Result<u64, String>>,
    poll_entered: mpsc::Receiver<Instant>,
    poll_started: Instant,
) -> Result<super::super::super::worker::ReadinessWorker, HostError> {
    let state = host.state.clone();
    let sample_journal = host.fixture.journal.clone();
    let journal = host.fixture.journal.clone();
    let engine_id = host.selected.engine_id().to_owned();
    let docker = host.docker.client().clone();
    let selected = host.selected.clone();
    let control = host.control.clone();
    let binding = host.binding.clone();
    let expected = host.fixture.runner.clone();
    let poll_entered = Arc::new(Mutex::new(Some(poll_entered)));
    spawn_refresh_worker(move || {
        let state = state.clone();
        let sample_journal = sample_journal.clone();
        let journal = journal.clone();
        let engine_id = engine_id.clone();
        let docker = docker.clone();
        let selected = selected.clone();
        let control = control.clone();
        let binding = binding.clone();
        let expected = expected.clone();
        let signal = signal.clone();
        let poll_entered = Arc::clone(&poll_entered);
        let poll_started = poll_started;
        async move {
            let poll_entered = poll_entered.lock().map_err(|_| HostError::Journal)?.take();
            let poll_entered_at = match poll_entered {
                Some(poll_entered) => match poll_entered.recv_timeout(Duration::from_secs(12)) {
                    Ok(entered_at) => entered_at,
                    Err(error) => {
                        let _sent = signal.send(Err(format!(
                            "The readiness refresh thread did not receive the poll-start signal. The wait limit is 12 seconds. Error: {error}."
                        )));
                        return Err(HostError::Ownership);
                    }
                },
                None => poll_started,
            };
            let result = collect_and_refresh(
                &state,
                &sample_journal,
                &journal,
                &engine_id,
                &docker,
                &selected,
                &control,
                &binding,
                &expected,
                poll_entered_at,
            )
            .await;
            match result {
                Ok(revision) => {
                    let _sent = signal.send(Ok(revision));
                    Ok(())
                }
                Err(failure) => {
                    let _sent = signal.send(Err(failure.diagnostic()));
                    Err(failure.error)
                }
            }
        }
    })
}

async fn collect_and_refresh(
    state: &std::path::Path,
    sample_journal: &crate::journal::Journal,
    journal: &crate::journal::Journal,
    engine_id: &str,
    docker: &bollard::Docker,
    selected: &SelectedEngineContext,
    control: &ControlState,
    binding: &RepositoryConnection,
    expected: &velnor_runner_github::RunnerReference,
    poll_started: Instant,
) -> Result<u64, RefreshFailure> {
    let sample_started = poll_started.elapsed();
    let sample_result = crate::launch::collect_sample_for_engine(sample_journal, engine_id).await;
    let sample_timing = SampleTiming {
        started_after_poll: sample_started,
        finished_after_poll: poll_started.elapsed(),
    };
    let sample = sample_result.map_err(|error| {
        refresh_failure(
            "sample_lifecycle",
            HostError::Docker,
            Some(error.to_string()),
            sample_timing,
        )
    })?;
    control
        .cache_readiness_sample(sample)
        .map_err(|error| refresh_failure("cache_sample", error, None, sample_timing))?;
    let remote_binding = binding.clone();
    let remote_expected = expected.clone();
    let remote = move |actual_binding: &RepositoryConnection,
                       actual: Option<&velnor_runner_github::RunnerReference>,
                       set_id: i64| {
        if actual_binding == &remote_binding && actual == Some(&remote_expected) && set_id == 501 {
            Ok(())
        } else {
            Err(HostError::Ownership)
        }
    };
    refresh_with_dependencies(state, docker, journal, selected, control, None, &remote)
        .await
        .map_err(|error| refresh_failure("refresh_audit", error, None, sample_timing))?;
    let (_, config_digest) = crate::readiness_scan::controller_config(state)
        .ok_or_else(|| refresh_failure("config_read", HostError::Config, None, sample_timing))?;
    let instance = journal
        .instance_id()
        .await
        .map_err(|error| refresh_failure("journal_instance", error, None, sample_timing))?;
    let revision = journal
        .revision()
        .await
        .map_err(|error| refresh_failure("journal_revision", error, None, sample_timing))?;
    if !control
        .readiness_matches(
            &config_digest,
            selected.engine_id(),
            selected.docker_root_digest(),
            &instance,
            revision,
            std::time::Instant::now(),
        )
        .map_err(|error| refresh_failure("proof_check", error, None, sample_timing))?
    {
        return Err(refresh_failure(
            "post_refresh_proof_not_matching",
            HostError::Ownership,
            None,
            sample_timing,
        ));
    }
    Ok(revision)
}
