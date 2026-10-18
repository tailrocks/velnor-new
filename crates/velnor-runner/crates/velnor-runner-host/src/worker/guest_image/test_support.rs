use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::config::RepositoryConnection;
use crate::config_test_support::{basic_connection, host_config};
use crate::error::HostError;
use crate::journal::EngineContext;
use crate::launch::SelectedEngineContext;
use crate::launch::verified_images::{VerifiedWorkerImages, test_worker_images};
use crate::{HostConfig, worker};

use super::GuestImageFile;

/// Complete test-only inputs for the mandatory verified stage path.
pub(crate) struct TestGuestImage {
    _state: TestState,
    pub(crate) connection: RepositoryConnection,
    pub(crate) images: VerifiedWorkerImages,
    pub(crate) selected_engine: SelectedEngineContext,
    pub(crate) file: GuestImageFile,
}

impl TestGuestImage {
    pub(crate) fn new() -> Result<Self, HostError> {
        let state = TestState::new()?;
        let binding = basic_connection(
            "build-main",
            42,
            "acme/project",
            "cache-account",
            "linux-build",
        );
        let config = HostConfig::parse(&host_config(&[binding], 1))?;
        let connection = config
            .connections()
            .first()
            .cloned()
            .ok_or(HostError::Config)?;
        let images = test_worker_images();
        let selected = EngineContext {
            engine_id: images.engine_id().to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            platform: images.platform().to_owned(),
        };
        let selected_engine = SelectedEngineContext::bind(&selected, &images)?;
        let file = worker::guest_image::write_guest_image(&state.0, &connection, &images)?;
        Ok(Self {
            _state: state,
            connection,
            images,
            selected_engine,
            file,
        })
    }
}

struct TestState(PathBuf);

impl TestState {
    fn new() -> Result<Self, HostError> {
        let path = std::env::temp_dir().join(format!("velnor-stage-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).map_err(|_| HostError::Path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
        Ok(Self(path))
    }
}

impl Drop for TestState {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}
