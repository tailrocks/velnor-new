use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::journal::RequestAcquireState;
use crate::launch::inspect_tests::DockerStub;
use crate::launch::resource_probe::StartPermit;
use crate::worker::Started;
use velnor_runner_github::Method;

use super::super::super::acquire::{self, AcquireDisposition};
use super::super::{reserve, start};
use super::support::{FakeLane, Fixture, exchange, fixture_storage, selected_engine, test_images};

#[tokio::test]
async fn reservation_records_the_v16_hold_before_any_job_effect() -> Result<(), String> {
    let fixture = Fixture::open("provisioning-hold").await?;
    let permit = StartPermit::test_fixture();
    let storage = fixture_storage(&permit)?;

    let physical = reserve(&fixture.scope, permit, storage)
        .await
        .map_err(|error| error.to_string())?;

    assert!(physical.reservation_id > 0);
    assert!(physical.intent_id > 0);
    assert!(physical.runner_name.starts_with('v'));
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    assert!(
        fixture
            .scope
            .claim_runner_jit(physical.intent_id, &physical.runner_name)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn acquired_request_waits_for_v16_binding_before_jit_or_docker() -> Result<(), String> {
    let fixture = Fixture::open("scoped-unbound-v16").await?;
    let permit = StartPermit::test_fixture();
    let storage = fixture_storage(&permit)?;
    let root_digest = permit.docker_root_digest().to_owned();
    let physical = reserve(&fixture.scope, permit, storage)
        .await
        .map_err(|error| error.to_string())?;

    let mut lane = FakeLane::new([Ok(exchange(200, r#"{"count":1,"value":[41]}"#))]);
    assert_eq!(
        acquire::run(&mut lane, "queue-token", &fixture.scope, &fixture.request,)
            .await
            .map_err(|error| error.to_string())?,
        AcquireDisposition::Acquired
    );

    let images = test_images(&root_digest);
    let selected = selected_engine(&images)?;
    let docker = DockerStub::open(Vec::new())?;
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    let result = start(
        &mut lane,
        "admin-token",
        &fixture.scope,
        &physical,
        &docker.docker,
        &selected,
        &images,
        move |_, _, _, _| {
            callback_calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(Started {
                runner_id: "unexpected-runner".to_owned(),
                dind_id: "unexpected-dind".to_owned(),
            }))
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(lane.methods, [Method::Post]);
    assert!(lane.paths[0].ends_with("/acquirejobs"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(docker.finish().await?.is_empty());
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Acquired)
    );
    assert!(
        fixture
            .scope
            .claim_runner_jit(physical.intent_id, &physical.runner_name)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}
