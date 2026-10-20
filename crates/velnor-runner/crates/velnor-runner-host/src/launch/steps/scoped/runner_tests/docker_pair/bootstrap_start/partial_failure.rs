use super::*;

impl BootstrapSetup {
    pub(super) async fn open() -> Result<Self, String> {
        Self::open_with_runner_create_status(None).await
    }

    async fn open_with_runner_create_status(
        runner_create_status: Option<u16>,
    ) -> Result<Self, String> {
        let fixture = Fixture::open_with_engine("native-bootstrap-pair", ENGINE_ID).await?;
        configure_capacity(&fixture).await?;
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
        let state = BootstrapAdmissionState::new(&fixture.binding)?;
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
    let setup = BootstrapSetup::open_with_runner_create_status(Some(409)).await?;
    let error = setup.start().await.err().ok_or_else(|| {
        "The Runner container request succeeded, but the test expected it to fail.".to_owned()
    })?;
    Ok(RunnerCreateFailure { setup, error })
}

impl RunnerCreateFailure {
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
        let captured = self.finish().await?;
        if let Some(error) = captured.server_error {
            return Err(format!("The Docker fixture failed: {error}"));
        }
        let probe = captured
            .requests
            .iter()
            .find(|request| is_probe_create(request))
            .ok_or_else(|| "The Capacity probe create request is absent.".to_owned())?;
        let response: serde_json::Value =
            serde_json::from_slice(&probe.response_body).map_err(|error| error.to_string())?;
        let id = response["Id"]
            .as_str()
            .ok_or_else(|| "The Capacity probe response has no container ID.".to_owned())?;
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
                != crate::launch::resource_probe::canonical_logs_response(1_203, 1_u64 << 30)
                    .map_err(|error| error.to_string())?
        {
            return Err(
                "The Capacity probe must read one canonical record and remove its container."
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
