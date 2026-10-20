use super::*;

#[path = "partial_failure/storage_claim_prefix.rs"]
mod storage_claim_prefix;

impl BootstrapSetup {
    pub(super) async fn open() -> Result<Self, String> {
        Self::open_with_runner_create_status(None).await
    }

    async fn open_with_runner_create_status(
        runner_create_status: Option<u16>,
    ) -> Result<Self, String> {
        Self::open_with_runner_create_status_and_jobs(runner_create_status, 1).await
    }

    async fn open_with_runner_create_status_and_jobs(
        runner_create_status: Option<u16>,
        _jobs: u32,
    ) -> Result<Self, String> {
        let fixture = Fixture::open_with_engine("native-bootstrap-pair", ENGINE_ID).await?;
        configure_capacity(&fixture).await?;
        let authority = crate::config::HostConfig::test_with_job_authority_policy(&fixture.binding)
            .map_err(|error| error.to_string())?;
        fixture
            .scope
            .install_job_authority_policy(&authority)
            .await
            .map_err(|error| error.to_string())?;
        let permit = StartPermit::test_fixture_with_engine_and_available_memory(
            &"a".repeat(32),
            ENGINE_ID,
            MEMORY_BYTES,
        );
        let images = super::super::super::support::test_images_for_engine(
            ENGINE_ID,
            permit.docker_root_digest(),
        );
        let engine = selected_engine(&images)?;
        let state = BootstrapAdmissionState::new(&fixture.binding, 1)?;
        let drive = bootstrap_drive(&fixture, &state.path);
        let poll = crate::launch_harness::available(&[REQUEST_ID]);
        let outcome = acquire_source_authorized(&drive, permit, &engine, &poll).await?;
        let (initializer_image, initializer) = initializer_for(&outcome, &fixture, &images).await?;
        let api = match runner_create_status {
            Some(status) => PairDockerApi::open_with_runner_create_rejection(
                images.clone(),
                initializer_image,
                initializer,
                status,
            )?,
            None => PairDockerApi::open_with_initializer(
                images.clone(),
                initializer_image,
                initializer,
            )?,
        };
        let prepared = prepare_acquired(outcome, &fixture, &api, &engine, &images).await?;
        let network = crate::launch::bootstrap::create_owned_network(
            &api.docker,
            &fixture.scope,
            &engine,
            prepared.acquired().reservation_id(),
        )
        .await
        .map_err(|error| format!("The private network preparation failed. Error: {error}."))?;
        Ok(Self {
            fixture,
            api,
            prepared,
            network,
            engine,
            images,
        })
    }
}

pub(crate) struct RunnerCreateFailure {
    setup: BootstrapSetup,
    error: String,
}

pub(crate) async fn runner_create_409() -> Result<RunnerCreateFailure, String> {
    runner_create_409_with_jobs(1).await
}

pub(crate) async fn runner_create_409_with_jobs(jobs: u32) -> Result<RunnerCreateFailure, String> {
    let setup = BootstrapSetup::open_with_runner_create_status_and_jobs(Some(409), jobs).await?;
    let error = setup.start().await.err().ok_or_else(|| {
        "The Runner container request succeeded, but the test expected it to fail.".to_owned()
    })?;
    if jobs > 1 {
        configure_capacity_for_jobs(&setup.fixture, jobs).await?;
    }
    Ok(RunnerCreateFailure { setup, error })
}

impl RunnerCreateFailure {
    pub(crate) fn binding(&self) -> &crate::config::RepositoryConnection {
        &self.setup.fixture.binding
    }
    pub(crate) fn set_probe_available_bytes(&self, available_bytes: u64) {
        self.setup.api.set_probe_available_bytes(available_bytes);
    }

    pub(crate) async fn storage_volume_names(&self) -> Result<Vec<String>, String> {
        let connection = self
            .setup
            .fixture
            .journal
            .connection()
            .await
            .map_err(|error| error.to_string())?;
        let mut rows = connection
            .query(
                "SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 ORDER BY role",
                [self.reservation_id()],
            )
            .await
            .map_err(|error| error.to_string())?;
        let mut names = Vec::new();
        while let Some(row) = rows.next().await.map_err(|error| error.to_string())? {
            names.push(row.get(0).map_err(|error| error.to_string())?);
        }
        Ok(names)
    }

    pub(crate) async fn journaled_volume_expectations(
        &self,
    ) -> Result<Vec<(String, String, i64)>, String> {
        let connection = self
            .setup
            .fixture
            .journal
            .connection()
            .await
            .map_err(|error| error.to_string())?;
        let mut rows = connection
            .query(
                "SELECT objects.docker_name, objects.role, provisioning.operation_id FROM capacity_reservation_provisioning_objects AS objects JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = objects.reservation_id WHERE objects.reservation_id = ?1 ORDER BY objects.role",
                [self.reservation_id()],
            )
            .await
            .map_err(|error| error.to_string())?;
        let mut expected = Vec::new();
        while let Some(row) = rows.next().await.map_err(|error| error.to_string())? {
            expected.push((
                row.get::<String>(0).map_err(|error| error.to_string())?,
                row.get::<String>(1).map_err(|error| error.to_string())?,
                row.get::<i64>(2).map_err(|error| error.to_string())?,
            ));
        }
        if expected.len() != 5 {
            return Err(
                "The acquired reservation must retain five journaled storage identities."
                    .to_owned(),
            );
        }
        expected.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(expected)
    }

    pub(crate) fn assert_journaled_volume_create_requests(
        expected: &[(String, String, i64)],
        connection_id: &str,
        reservation_id: i64,
        requests: &[CapturedRequest],
    ) -> Result<(), String> {
        let mut actual = requests
            .iter()
            .filter(|request| {
                request.method == "POST" && request.target.starts_with("/volumes/create")
            })
            .map(|request| {
                let body: serde_json::Value =
                    serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
                Ok((
                    body["Name"]
                        .as_str()
                        .ok_or_else(|| "A volume create omitted its name.".to_owned())?
                        .to_owned(),
                    body["Labels"].clone(),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        actual.sort_by(|left, right| left.0.cmp(&right.0));
        if actual.len() != expected.len() {
            return Err("Bootstrap must create each journaled storage volume once.".to_owned());
        }
        for ((name, role, operation_id), (actual_name, labels)) in expected.iter().zip(actual) {
            let expected_labels = serde_json::json!({
                "velnor.connection": connection_id,
                "velnor.reservation": reservation_id.to_string(),
                "velnor.operation": operation_id.to_string(),
                "velnor.role": role.as_str(),
            });
            if name != &actual_name || labels != expected_labels {
                return Err(format!(
                    "The volume create for {name} did not use its exact journaled identity: {actual_name} {labels:?} {expected_labels}."
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn inject_volume_delete_transport_failure(&self, name: &str) {
        CapturedRequest::inject_volume_delete_transport_failure(name);
    }

    pub(crate) fn inject_volume_inspect_transport_failure(&self, name: &str) {
        CapturedRequest::inject_volume_inspect_transport_failure(name);
    }

    pub(crate) fn host_journal(&self) -> &crate::journal::Journal {
        &self.setup.fixture.journal
    }

    pub(crate) fn scope(&self) -> &crate::journal::ConnectionJournal {
        &self.setup.fixture.scope
    }

    pub(crate) fn docker(&self) -> &bollard::Docker {
        &self.setup.api.docker
    }

    pub(crate) fn engine(&self) -> &crate::launch::SelectedEngineContext {
        &self.setup.engine
    }

    pub(crate) fn images(&self) -> &crate::launch::VerifiedWorkerImages {
        &self.setup.images
    }

    pub(crate) fn reservation_id(&self) -> i64 {
        self.setup.prepared.acquired().reservation_id()
    }

    pub(crate) fn assignment(&self) -> &crate::journal::AssignmentIdentity {
        self.setup.prepared.acquired().physical_assignment()
    }

    pub(crate) fn error(&self) -> &str {
        &self.error
    }

    pub(crate) fn journal_file(&self) -> std::path::PathBuf {
        self.setup.fixture.journal_file()
    }

    pub(crate) async fn finish(self) -> Result<CapturedApi, String> {
        self.setup.api.finish_partial_start().await
    }

    pub(crate) async fn finish_and_assert_capacity_probe(self) -> Result<(), String> {
        let available_bytes = self.setup.api.probe_available_bytes();
        let captured = self.finish().await?;
        if let Some(error) = captured.server_error {
            return Err(format!("The Docker fixture failed. Error: {error}."));
        }
        let probe = captured
            .requests
            .iter()
            .find(|request| is_probe_create(request))
            .ok_or_else(|| "The capacity probe create request is missing.".to_owned())?;
        let response: serde_json::Value =
            serde_json::from_slice(&probe.response_body).map_err(|error| error.to_string())?;
        let id = response["Id"]
            .as_str()
            .ok_or_else(|| "The capacity probe response has no container ID.".to_owned())?;
        let logs: Vec<_> = captured
            .requests
            .iter()
            .filter(|request| {
                request.method == "GET"
                    && request.target.contains(&format!("/containers/{id}/logs"))
            })
            .collect();
        let deletes = captured
            .requests
            .iter()
            .filter(|request| {
                request.method == "DELETE" && request.target.contains(&format!("/containers/{id}"))
            })
            .count();
        if logs.len() != 1
            || deletes != 1
            || logs[0].response_body
                != crate::launch::resource_probe::canonical_logs_response(1_203, available_bytes)
                    .map_err(|error| error.to_string())?
        {
            return Err(
                "The test must read one canonical record from the capacity probe and remove its container."
                    .to_owned(),
            );
        }
        let network_deletes = captured
            .requests
            .iter()
            .filter(|request| {
                request.method == "DELETE"
                    && request.target.ends_with("/networks/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")
            })
            .count();
        if network_deletes != 1 {
            return Err("The cleanup did not remove the private network by the ID in the inspection response.".to_owned());
        }
        Ok(())
    }
}

fn is_probe_create(request: &CapturedRequest) -> bool {
    request.method == "POST"
        && route(request).ends_with("/containers/create")
        && serde_json::from_slice::<serde_json::Value>(&request.body)
            .ok()
            .is_some_and(|body| body["Labels"]["velnor.role"] == "resource-probe")
}
