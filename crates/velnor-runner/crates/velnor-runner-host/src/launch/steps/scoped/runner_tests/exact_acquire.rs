use super::*;

#[tokio::test]
async fn exact_acquire_jit_and_bounded_docker_preflight_use_one_physical_nonce()
-> Result<(), String> {
    const ENGINE_ID: &str = crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID;
    let fixture = Fixture::open_with_engine("scoped-exact", ENGINE_ID).await?;
    fixture
        .journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 4_000,
                memory_bytes: 32 * 1024 * 1024 * 1024,
            },
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 256 * 1024 * 1024,
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    let profile = fixture_provisioning_profile();
    fixture
        .journal
        .configure_storage_profile(profile.clone())
        .await
        .map_err(|error| error.to_string())?;
    let permit = StartPermit::test_fixture_with_engine_and_available_memory(
        &"a".repeat(32),
        ENGINE_ID,
        32 * 1024 * 1024 * 1024,
    );
    let storage = permit
        .storage_provisioning_admission(&profile, permit.engine_id(), permit.docker_root_digest())
        .ok_or_else(|| {
            "The verified test sample does not meet storage admission rules.".to_owned()
        })?;
    let root_digest = permit.docker_root_digest().to_owned();
    let engine_id = permit.engine_id().to_owned();
    let reserved = runner::reserve_and_claim(
        &fixture.scope,
        permit,
        storage,
        &fixture.request,
        &fixture.authority_policy,
    )
    .await
    .map_err(|error| error.to_string())?;
    let ReservationOutcome::AcquireClaimed(physical, claim) = reserved else {
        return Err("The authorized request did not receive one atomic Acquire claim.".to_owned());
    };
    assert_runner_name(&physical.runner_name)?;

    let mut lane = FakeLane::new([
        Ok(exchange(200, r#"{"count":1,"value":[41]}"#)),
        Ok(exchange(
            200,
            &json!({
                "encodedJITConfig": "opaque-jit",
                "runner": {
                    "id": 9001,
                    "name": physical.runner_name,
                    "runnerScaleSetId": SET_ID
                }
            })
            .to_string(),
        )),
    ]);
    assert_eq!(
        acquire::run_claimed(&mut lane, "queue-token", &fixture.scope, claim)
            .await
            .map_err(|error| error.to_string())?,
        AcquireDisposition::Acquired
    );
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Acquired)
    );

    let nonce = physical
        .runner_name
        .strip_prefix('v')
        .ok_or_else(|| "The runner name is missing its version prefix.".to_owned())?;
    crate::journal::request_lifecycle::tests::helpers::provision_acquired_physical_intent(
        &fixture.scope,
        &engine_id,
        physical.assignment.clone(),
        nonce,
        physical.reservation_id,
        physical.intent_id,
        root_digest.clone(),
        physical.sample_generation_id.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;

    let images = test_images_for_engine(ENGINE_ID, &root_digest);
    let selected = selected_engine(&images)?;
    let docker = docker_identity_stub(&images)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    let started = runner::start(
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
                runner_id: "runner-container".to_owned(),
                dind_id: "dind-container".to_owned(),
            }))
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(started.runner_id, "runner-container");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let requests = docker.finish().await?;
    assert_eq!(requests.len(), 8);
    assert!(
        requests
            .iter()
            .filter(|request| request.contains("/info"))
            .count()
            == 4
    );
    assert!(
        requests
            .iter()
            .filter(|request| request.contains("/images/"))
            .count()
            == 4
    );
    assert_eq!(lane.methods, [Method::Post, Method::Post]);
    assert!(lane.paths[0].ends_with("/acquirejobs"));
    assert!(lane.paths[1].contains("/generatejitconfig"));
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    assert!(
        fixture
            .scope
            .claim_runner_jit_for_request(
                physical.intent_id,
                physical.reservation_id,
                fixture
                    .request
                    .request_id()
                    .ok_or_else(|| "The request ID is missing.".to_owned())?,
                &physical.runner_name,
                physical.authority_generation,
                &physical.authority_digest,
            )
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}
