use super::*;
use std::time::{Duration, Instant};

use crate::launch::resource_probe::ReleaseReadSecret;
use velnor_runner_github::Exchange;
use velnor_runner_github::Poll;

const ACTUAL_STARTED_REQUEST_ID: i64 = 42;

fn actual_started_request_id() -> i64 {
    ACTUAL_STARTED_REQUEST_ID
}

#[path = "native_intake_composition/started_replay.rs"]
mod started_replay;
use started_replay::{listener_failure, record_started_and_replay};
#[path = "native_intake_composition/quarantine_inbox.rs"]
mod quarantine_inbox;
use quarantine_inbox::verify_quarantined_inbox;
#[path = "native_intake_composition/pre_delivery.rs"]
mod pre_delivery;
use pre_delivery::assert_pre_delivery_snapshot;
#[path = "native_intake_composition/ack_recovery.rs"]
mod ack_recovery;
#[path = "native_intake_composition/pair_assertions.rs"]
mod pair_assertions;
#[path = "native_intake_composition/tampered_channel.rs"]
mod tampered_channel;
use pair_assertions::{assert_bound_identity, assert_jit_persistence, verify_native_pair_requests};

#[test]
fn acquired_request_reaches_measured_storage_bound() -> Result<(), String> {
    const PRODUCTION_CALLER_STACK_BYTES: usize = 8 * 1024 * 1024;
    std::thread::Builder::new()
        .name("native-intake-production-stack".to_owned())
        .stack_size(PRODUCTION_CALLER_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(acquired_request_reaches_measured_storage_bound_inner())
        })
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "The production-stack composition thread panicked.".to_owned())?
}

async fn acquired_request_reaches_measured_storage_bound_inner() -> Result<(), String> {
    let setup = Box::pin(bootstrap_start::open_composition()).await?;
    Box::pin(continue_native_composition(setup, None)).await
}

pub(crate) fn run_native_quarantine_composition(
    polled: Poll,
    backlog_rows: usize,
) -> Result<(), String> {
    const PRODUCTION_CALLER_STACK_BYTES: usize = 8 * 1024 * 1024;
    std::thread::Builder::new()
        .name("native-quarantine-production-stack".to_owned())
        .stack_size(PRODUCTION_CALLER_STACK_BYTES)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(async move {
                let setup = Box::pin(bootstrap_start::open_composition_with_poll(
                    &polled,
                    backlog_rows,
                ))
                .await?;
                Box::pin(continue_native_composition(
                    setup,
                    Some((&polled, backlog_rows)),
                ))
                .await
            })
        })
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "The native intake test thread stopped unexpectedly.".to_owned())?
}

async fn continue_native_composition(
    mut setup: bootstrap_start::CompositionSetup,
    quarantined: Option<(&Poll, usize)>,
) -> Result<(), String> {
    let request_id = setup
        .fixture
        .request
        .request_id()
        .ok_or_else(|| "The acquired request has no native ID.".to_owned())?;
    let provider = crate::launch::resource_probe::IntakeCompositionProvider::new(
        &setup.engine,
        setup.images.source_sha(),
    );
    let outcome = setup
        .outcome
        .take()
        .ok_or_else(|| "The Acquire outcome was already consumed.".to_owned())?;
    let bound = Box::pin(bind_acquired(&setup, &provider, outcome)).await?;
    let reservation_id = bound.prepared().acquired().reservation_id();
    let resource_budget = bound.prepared().acquired().resource_budget();
    assert_bound_identity(&setup, &bound, request_id).await?;
    let ready = Box::pin(jit_and_deliver(&setup, bound, request_id, &provider))
        .await
        .map_err(|error| format!("The JIT or listener step failed: {error}."))?;
    assert_eq!(ready.request_id(), Some(request_id));
    assert_eq!(ready.runner().id, 9001);
    bootstrap_start::authorize_actual_started_request(&setup.fixture, actual_started_request_id())
        .await?;
    assert_jit_persistence(&setup.fixture, ready.intent_id(), &ready.runner().name)
        .await
        .map_err(|error| format!("The saved JIT state check failed: {error}."))?;
    let network = setup
        .fixture
        .scope
        .storage_network_recovery_identity(reservation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The owned storage network record is absent.".to_owned())?;
    let network_name = network.network_name().to_owned();
    let network_id = network
        .network_id()
        .ok_or_else(|| "The owned storage network ID is absent.".to_owned())?
        .to_owned();
    let network_labels = network.labels().clone();
    let before_started = setup.api.activity_snapshot()?;
    record_started_and_replay(&setup.fixture, actual_started_request_id(), &ready)
        .await
        .map_err(|error| format!("Started/reopen replay: {error}"))?;
    if let Some((polled, backlog_rows)) = quarantined {
        verify_quarantined_inbox(&setup.fixture, polled, backlog_rows).await?;
        if let Some(poll_session) = setup.poll_session.take() {
            let reopened = setup.fixture.reopen().await?;
            bootstrap_start::replay_owned_terminal_offer(
                reopened,
                poll_session,
                request_id,
                actual_started_request_id(),
            )
            .await?;
        } else {
            return Err("The mixed-poll replay session is absent.".to_owned());
        }
    }
    let after_replay = setup.api.activity_snapshot()?;
    assert_eq!(before_started, after_replay);
    assert!(before_started.publisher_containers > 0);
    assert_eq!(
        before_started.publisher_containers,
        before_started.publisher_logs
    );
    assert_eq!(before_started.runner_attaches, 1);
    drop(ready);
    let evidence = setup.api.finish().await?;
    assert!(
        evidence.server_error.is_none(),
        "{:?}",
        evidence.server_error
    );
    assert_eq!(evidence.jit_input, b"opaque-intake-jit");
    verify_native_pair_requests(
        &evidence.requests,
        &network_name,
        &network_id,
        &network_labels,
        resource_budget,
        &setup.images,
        &evidence.jit_input,
    )?;
    Ok(())
}

async fn bind_acquired<P: crate::launch::resource_probe::ImageProvider>(
    setup: &bootstrap_start::CompositionSetup,
    provider: &P,
    outcome: crate::launch::steps::ScopedProvisioningOutcome,
) -> Result<crate::launch::scoped_runtime::BoundScopedBootstrap, String> {
    let result = crate::launch::scoped_runtime::prepare_and_bind_scoped_with_test_provider(
        outcome,
        &setup.api.docker,
        &setup.fixture.scope,
        &setup.fixture.journal,
        provider,
        &setup.engine,
        &setup.images,
    )
    .await
    .map_err(|error| format!("The production bind step failed: {error}."))?;
    match result {
        crate::launch::scoped_runtime::ScopedRuntimeOutcome::Bound(bound) => Ok(*bound),
        crate::launch::scoped_runtime::ScopedRuntimeOutcome::Pending(pending) => {
            pending_bind_error(*pending)
        }
        crate::launch::scoped_runtime::ScopedRuntimeOutcome::NoStart => {
            Err("The production bind step returned NoStart.".to_owned())
        }
    }
}

fn pending_bind_error(
    pending: crate::launch::scoped_runtime::PendingScopedBootstrap,
) -> Result<crate::launch::scoped_runtime::BoundScopedBootstrap, String> {
    let failure = pending.failure().to_string();
    drop(pending);
    Err(format!(
        "The production bind step returned Pending: {failure}."
    ))
}

async fn jit_and_deliver<P: crate::launch::resource_probe::ImageProvider>(
    setup: &bootstrap_start::CompositionSetup,
    bound: crate::launch::scoped_runtime::BoundScopedBootstrap,
    request_id: i64,
    provider: &P,
) -> Result<crate::launch::scoped_runtime::RunnerReadyBootstrap, String> {
    let channel = create_and_measure_channel(setup, bound).await?;
    let setup_ref = setup;
    let bound_intent_id = channel.bound().prepared().acquired().intent_id();
    let binding_digest = channel
        .bound()
        .binding()
        .instance_binding_digest()
        .to_owned();
    let secret = ReleaseReadSecret::test_token();
    let host_context = crate::launch::scoped_runtime::ScopedHostContext {
        docker: &setup.api.docker,
        host: &setup.fixture.journal,
        scoped: &setup.fixture.scope,
        secret: &secret,
        engine: &setup.engine,
        images: &setup.images,
    };
    let ready = crate::launch::scoped_runtime::prepare_listener_with_test_provider(
        channel,
        &host_context,
        &setup.guest_image.connection,
        &setup.guest_image.file,
        provider,
        move |reservation_id| async move {
            if let Err(error) = assert_pre_delivery_snapshot(
                setup_ref,
                reservation_id,
                bound_intent_id,
                &binding_digest,
            )
            .await
            {
                panic!("pre-delivery snapshot: {error}");
            }
            assert_eq!(
                setup_ref
                    .api
                    .activity_snapshot()
                    .map_err(|_| crate::error::HostError::Docker)?
                    .jit_input_bytes,
                0
            );
            Ok::<(), crate::error::HostError>(())
        },
        Instant::now() + Duration::from_secs(30),
    )
    .await;
    let ready = match ready {
        Ok(ready) => ready,
        Err(pending) => {
            let requests = setup.api.request_summary()?;
            return Err(format!(
                "The production listener step failed: {}. Requests: {requests:?}",
                listener_failure(pending)
            ));
        }
    };
    if ready.request_id() != Some(request_id) {
        return Err("The listener delivery lost its acquired request identity.".to_owned());
    }
    Ok(ready)
}

async fn create_and_measure_channel(
    setup: &bootstrap_start::CompositionSetup,
    bound: crate::launch::scoped_runtime::BoundScopedBootstrap,
) -> Result<crate::launch::scoped_runtime::ChannelReadyBootstrap, String> {
    let acquired = bound.prepared().acquired();
    let response = Exchange {
        status: 200,
        body: serde_json::json!({
            "encodedJITConfig": "opaque-intake-jit",
            "runner": {
                "id": 9001,
                "name": acquired.runner_name(),
                "runnerScaleSetId": acquired.request().scale_set_id()
            }
        })
        .to_string()
        .into_bytes(),
    };
    let mut lane = FakeLane::new([Ok(response)]);
    let jit = crate::launch::scoped_runtime::create_and_bind(
        bound,
        &mut lane,
        "test-admin-token",
        &setup.fixture.scope,
    )
    .await
    .map_err(|pending| {
        format!(
            "The production JIT bind step failed: {:?}.",
            pending.failure()
        )
    })?;
    assert_eq!(lane.paths.len(), 1);
    assert!(lane.paths[0].contains("/generatejitconfig"));
    Box::pin(jit.measure_channel(
        &setup.api.docker,
        &setup.engine,
        &setup.images,
        Instant::now() + Duration::from_secs(30),
    ))
    .await
    .map_err(|pending| {
        format!(
            "The production channel measurement failed: {:?}.",
            pending.failure()
        )
    })
}
