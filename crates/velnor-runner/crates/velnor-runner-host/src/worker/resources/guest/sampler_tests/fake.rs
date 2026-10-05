use std::future::pending;
use std::sync::Arc;
use std::time::Duration;

use bollard::models::SystemInfo;
use tokio::sync::Mutex;

use crate::journal::LaunchIdentity;

use super::super::super::GuestResourceSample;
use super::super::identity::{IMAGE_REFERENCE, container_name, owner_labels};
use super::super::{
    CreatedProbe, GuestDockerClient, GuestSampleFailure, ProbeContainer, ProbeImage,
};

pub(super) const ENGINE_ID: &str = "engine-guest-a";
pub(super) const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const RESOLVED_IMAGE: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const REPLACEMENT_IMAGE: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
pub(super) const VALID_OUTPUT: &[u8] =
    b"mem_available_kib=2048\nmemory_psi_some_avg10=1.25\ndocker_root_free_kib=4096\n";

#[derive(Debug)]
pub(super) struct FakeClient {
    pub(super) state: Mutex<FakeState>,
}

#[derive(Debug)]
pub(super) struct FakeState {
    pub(super) image: Option<ProbeImage>,
    pub(super) replacement_image_after_lookup: Option<ProbeImage>,
    pub(super) image_error: bool,
    pub(super) image_hangs: bool,
    pub(super) info_hangs: bool,
    pub(super) create_hangs: bool,
    pub(super) create_loses_response: bool,
    pub(super) start_hangs: bool,
    pub(super) output_hangs: bool,
    pub(super) remove_hangs: bool,
    pub(super) remove_keeps_container: bool,
    pub(super) wrong_engine: bool,
    pub(super) container: Option<ProbeContainer>,
    pub(super) output: Vec<u8>,
    pub(super) image_reference: Option<String>,
    pub(super) created_image: Option<String>,
    pub(super) created_name: Option<String>,
    pub(super) create_count: usize,
    pub(super) start_count: usize,
    pub(super) remove_count: usize,
}

impl FakeClient {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(FakeState {
                image: Some(image(RESOLVED_IMAGE)),
                replacement_image_after_lookup: None,
                image_error: false,
                image_hangs: false,
                info_hangs: false,
                create_hangs: false,
                create_loses_response: false,
                start_hangs: false,
                output_hangs: false,
                remove_hangs: false,
                remove_keeps_container: false,
                wrong_engine: false,
                container: None,
                output: VALID_OUTPUT.to_vec(),
                image_reference: None,
                created_image: None,
                created_name: None,
                create_count: 0,
                start_count: 0,
                remove_count: 0,
            }),
        }
    }

    pub(super) async fn seed_container(&self, container: ProbeContainer) {
        self.state.lock().await.container = Some(container);
    }
}

impl GuestDockerClient for FakeClient {
    async fn info(&self) -> Result<SystemInfo, GuestSampleFailure> {
        let state = self.state.lock().await;
        if state.info_hangs {
            drop(state);
            return pending().await;
        }
        let id = if state.wrong_engine {
            "another-engine"
        } else {
            ENGINE_ID
        };
        Ok(SystemInfo {
            id: Some(id.to_owned()),
            ncpu: Some(4),
            docker_root_dir: Some("/srv/docker-data".to_owned()),
            ..Default::default()
        })
    }

    async fn inspect_image(
        &self,
        reference: &str,
    ) -> Result<Option<ProbeImage>, GuestSampleFailure> {
        let mut state = self.state.lock().await;
        state.image_reference = Some(reference.to_owned());
        if state.image_hangs {
            drop(state);
            return pending().await;
        }
        if state.image_error {
            return Err(GuestSampleFailure::Docker);
        }
        let resolved = state.image.clone();
        if let Some(replacement) = state.replacement_image_after_lookup.take() {
            state.image = Some(replacement);
        }
        Ok(resolved)
    }

    async fn inspect_container(
        &self,
        _name: &str,
    ) -> Result<Option<ProbeContainer>, GuestSampleFailure> {
        Ok(self.state.lock().await.container.clone())
    }

    async fn create_container(
        &self,
        request: super::super::super::GuestProbeCreate,
    ) -> Result<CreatedProbe, GuestSampleFailure> {
        let mut state = self.state.lock().await;
        state.create_count += 1;
        state.created_image = request.config.image.clone();
        state.created_name = request.options.name.clone();
        let name = request
            .options
            .name
            .ok_or(GuestSampleFailure::ProbeCreate)?;
        let labels = request
            .config
            .labels
            .ok_or(GuestSampleFailure::ProbeCreate)?;
        state.container = Some(ProbeContainer {
            id: Some(ID.to_owned()),
            name: Some(format!("/{name}")),
            image: request.config.image,
            labels,
            running: Some(true),
            exit_code: None,
        });
        let create_hangs = state.create_hangs;
        let loses_response = state.create_loses_response;
        drop(state);
        if create_hangs {
            return pending().await;
        }
        if loses_response {
            return Err(GuestSampleFailure::Docker);
        }
        Ok(CreatedProbe {
            id: ID.to_owned(),
            has_warnings: false,
        })
    }

    async fn start_container(&self, _id: &str) -> Result<(), GuestSampleFailure> {
        let mut state = self.state.lock().await;
        state.start_count += 1;
        if let Some(container) = &mut state.container {
            container.running = Some(false);
            container.exit_code = Some(0);
        }
        let start_hangs = state.start_hangs;
        drop(state);
        if start_hangs {
            return pending().await;
        }
        Ok(())
    }

    async fn read_output(&self, _id: &str, _limit: usize) -> Result<Vec<u8>, GuestSampleFailure> {
        let state = self.state.lock().await;
        if state.output_hangs {
            drop(state);
            return pending().await;
        }
        Ok(state.output.clone())
    }

    async fn remove_container(&self, id: &str) -> Result<(), GuestSampleFailure> {
        let mut state = self.state.lock().await;
        state.remove_count += 1;
        if state.remove_hangs {
            drop(state);
            return pending().await;
        }
        if state.remove_keeps_container {
            return Ok(());
        }
        if state.container.as_ref().and_then(|item| item.id.as_deref()) != Some(id) {
            return Err(GuestSampleFailure::Docker);
        }
        state.container = None;
        Ok(())
    }
}

pub(super) fn identity() -> Result<LaunchIdentity, crate::error::HostError> {
    LaunchIdentity::new(
        "11111111111111111111111111111111",
        1,
        "22222222222222222222222222222222",
        ENGINE_ID,
    )
}

pub(super) fn image(id: &str) -> ProbeImage {
    ProbeImage {
        id: Some(id.to_owned()),
        repo_tags: vec![IMAGE_REFERENCE.to_owned()],
        os: Some("linux".to_owned()),
        architecture: Some("amd64".to_owned()),
    }
}

pub(super) fn owned_container(identity: &LaunchIdentity) -> ProbeContainer {
    ProbeContainer {
        id: Some(ID.to_owned()),
        name: Some(format!("/{}", container_name(identity))),
        image: Some(RESOLVED_IMAGE.to_owned()),
        labels: owner_labels(identity),
        running: Some(false),
        exit_code: Some(0),
    }
}

pub(super) fn fast_sampler<'a>(
    client: Arc<FakeClient>,
    identity: &'a LaunchIdentity,
) -> super::super::GuestSampler<'a, FakeClient> {
    super::super::GuestSampler::new(client, identity).with_timing(super::super::SamplerTiming {
        operation: Duration::from_millis(25),
        probe_run: Duration::from_millis(50),
        cleanup: Duration::from_millis(25),
        exit_poll: Duration::from_millis(1),
    })
}

pub(super) fn unavailable(
    snapshot: super::super::GuestSampleSnapshot,
    reason: GuestSampleFailure,
) -> bool {
    snapshot.status == super::super::GuestSampleStatus::Unavailable(reason)
        && snapshot.sample == GuestResourceSample::default()
}
