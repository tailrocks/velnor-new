use super::*;

pub(super) fn verify_native_pair_requests(
    requests: &[CapturedRequest],
    expected_network_name: &str,
    expected_network_id: &str,
    expected_network_labels: &std::collections::BTreeMap<String, String>,
    budget: crate::worker::ResourceBudget,
    images: &crate::launch::VerifiedWorkerImages,
    jit_input: &[u8],
) -> Result<(), String> {
    let network_creates = requests
        .iter()
        .filter(|request| request.method == "POST" && route(request) == "/networks/create")
        .collect::<Vec<_>>();
    assert_eq!(network_creates.len(), 1);
    let network_create = network_creates[0];
    let network_body: Value = serde_json::from_slice(&network_create.body)
        .map_err(|error| format!("The network create request is invalid: {error}."))?;
    assert_eq!(network_body["Name"], expected_network_name);
    assert_eq!(network_body["Driver"], "bridge");
    assert_eq!(network_body["Scope"], "local");
    assert_eq!(network_body["Internal"], false);
    assert_eq!(network_body["Attachable"], false);
    assert_eq!(network_body["Ingress"], false);
    assert_eq!(network_body["EnableIPv4"], true);
    assert_eq!(network_body["EnableIPv6"], false);
    assert_eq!(
        network_body["Labels"],
        serde_json::to_value(expected_network_labels).map_err(|error| error.to_string())?
    );
    let network_response: Value = serde_json::from_slice(&network_create.response_body)
        .map_err(|error| format!("The network create response is invalid: {error}."))?;
    assert_eq!(network_response["Id"], expected_network_id);
    super::request_checks::verify_worker_image_ids(requests, images)?;
    assert_jit_is_absent_from_docker_requests(requests, jit_input)?;
    assert_volume_roles(requests);
    super::request_checks::verify_pair_requests(requests, expected_network_name, budget, true)?;
    Ok(())
}

fn assert_jit_is_absent_from_docker_requests(
    requests: &[CapturedRequest],
    jit_input: &[u8],
) -> Result<(), String> {
    if jit_input.is_empty()
        || requests.iter().any(|request| {
            request
                .body
                .windows(jit_input.len())
                .any(|window| window == jit_input)
        })
    {
        return Err("Docker request JSON contains the JIT configuration.".to_owned());
    }
    Ok(())
}

fn assert_volume_roles(requests: &[CapturedRequest]) {
    let creates = requests
        .iter()
        .filter(|request| request.method == "POST" && route(request) == "/volumes/create")
        .collect::<Vec<_>>();
    assert_eq!(creates.len(), 5);
    let roles = creates
        .iter()
        .map(|request| {
            let body: Value =
                serde_json::from_slice(&request.body).expect("The volume request must be JSON.");
            body["Labels"]["velnor.role"]
                .as_str()
                .expect("The volume role label must be present.")
                .to_owned()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        roles,
        std::collections::BTreeSet::from([
            "dind-data".to_owned(),
            "engine-metadata".to_owned(),
            "runner-home".to_owned(),
            "runner-work".to_owned(),
            "shared-run".to_owned(),
        ])
    );
}

pub(super) async fn assert_bound_identity(
    setup: &bootstrap_start::CompositionSetup,
    bound: &crate::launch::scoped_runtime::BoundScopedBootstrap,
    request_id: i64,
) -> Result<(), String> {
    assert_eq!(
        bound.prepared().acquired().request().request_id(),
        Some(request_id)
    );
    assert_eq!(bound.started().runner_container_id(), RUNNER_ID);
    assert_eq!(bound.started().dind_container_id(), DIND_ID);
    let acquired = bound.prepared().acquired();
    let binding = bound.binding();
    assert_eq!(binding.reservation_id(), acquired.reservation_id());
    assert_eq!(binding.operation_id(), acquired.intent_id());
    assert_eq!(binding.engine_id(), setup.engine.engine_id());
    assert_eq!(
        binding.docker_root_digest(),
        setup.engine.docker_root_digest()
    );
    assert_eq!(binding.runner_container_id(), RUNNER_ID);
    assert_eq!(binding.dind_container_id(), DIND_ID);
    assert_bound_persistence(&setup.fixture, acquired.reservation_id(), request_id)
        .await
        .map_err(|error| format!("bound persistence: {error}"))
}

async fn assert_bound_persistence(
    fixture: &Fixture,
    reservation_id: i64,
    request_id: i64,
) -> Result<(), String> {
    let rows = fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == reservation_id)
        .ok_or_else(|| "Bound reservation is not durable.".to_owned())?;
    assert_eq!(
        row.launch_phase,
        Some(crate::reconcile::LaunchPhase::ResourcesBound)
    );
    assert_eq!(
        row.jit_state,
        crate::journal::PhysicalRunnerJitState::Absent
    );
    assert_eq!(row.runner_container_id.as_deref(), Some(RUNNER_ID));
    assert_eq!(row.dind_container_id.as_deref(), Some(DIND_ID));
    assert_eq!(
        row.acquired_request
            .as_ref()
            .and_then(crate::journal::AssignmentIdentity::request_id),
        Some(request_id)
    );
    Ok(())
}

pub(super) async fn assert_jit_persistence(
    fixture: &Fixture,
    intent_id: i64,
    runner_name: &str,
) -> Result<(), String> {
    let rows = fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.intent_id == intent_id)
        .ok_or_else(|| "JIT reservation is not durable.".to_owned())?;
    assert_eq!(row.jit_state, crate::journal::PhysicalRunnerJitState::Ready);
    assert_eq!(row.runner.as_ref().map(|runner| runner.id), Some(9001));
    assert_eq!(
        row.runner.as_ref().map(|runner| runner.name.as_str()),
        Some(runner_name)
    );
    let publication = fixture
        .scope
        .native_publication_snapshot(row.reservation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Listener delivery state is not durable.".to_owned())?;
    assert_eq!(publication.jit_state, "bound");
    assert_eq!(publication.publication_state, "published");
    assert_eq!(publication.challenge_state.as_deref(), Some("published"));
    assert_eq!(publication.delivery_state, "delivered");
    Ok(())
}
