use std::future::pending;
use std::sync::Arc;
use std::time::Duration;

use bollard::models::SystemInfo;
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::super::super::GuestResourceSample;
use super::super::identity::{IMAGE_REFERENCE, container_name, owner_labels};
use super::super::{
    CreatedProbe, GuestDockerClient, GuestProbeIdentity, GuestSampleFailure, ProbeContainer,
    ProbeImage,
};

pub(super) const ENGINE_ID: &str = "engine-guest-a";
pub(super) const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const OTHER_ID: &str =
    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
pub(super) const RESOLVED_IMAGE: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const REPLACEMENT_IMAGE: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
pub(super) const VALID_OUTPUT: &[u8] =
    b"mem_available_kib=2048\nmemory_psi_some_avg10=1.25\ndocker_root_free_kib=4096\n";

#[derive(Debug)]
pub(super) struct FakeClient {
    pub(super) state: Arc<Mutex<FakeState>>,
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
    pub(super) late_create_delay: Duration,
    pub(super) late_create_after_absence_count: usize,
    pub(super) start_hangs: bool,
    pub(super) output_hangs: bool,
    pub(super) output_not_ready_checks: usize,
    pub(super) output_read_count: usize,
    pub(super) remove_hangs: bool,
    pub(super) remove_keeps_container: bool,
    pub(super) rename_on_remove: bool,
    pub(super) wrong_engine: bool,
    pub(super) wrong_engine_after_start: bool,
    pub(super) switch_engine_after_id_inspect: bool,
    pub(super) wrong_exit_id: bool,
    pub(super) info_delay: Duration,
    pub(super) exit_inspect_delay: Duration,
    pub(super) remove_delay: Duration,
    pub(super) container: Option<ProbeContainer>,
    pub(super) output: Vec<u8>,
    pub(super) image_reference: Option<String>,
    pub(super) created_image: Option<String>,
    pub(super) created_name: Option<String>,
    pub(super) create_count: usize,
    pub(super) post_create_absence_count: usize,
    pub(super) start_count: usize,
    pub(super) remove_count: usize,
}

impl FakeClient {
    pub(super) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                image: Some(image(RESOLVED_IMAGE)),
                replacement_image_after_lookup: None,
                image_error: false,
                image_hangs: false,
                info_hangs: false,
                create_hangs: false,
                create_loses_response: false,
                late_create_delay: Duration::ZERO,
                late_create_after_absence_count: 0,
                start_hangs: false,
                output_hangs: false,
                output_not_ready_checks: 0,
                output_read_count: 0,
                remove_hangs: false,
                remove_keeps_container: false,
                rename_on_remove: false,
                wrong_engine: false,
                wrong_engine_after_start: false,
                switch_engine_after_id_inspect: false,
                wrong_exit_id: false,
                info_delay: Duration::ZERO,
                exit_inspect_delay: Duration::ZERO,
                remove_delay: Duration::ZERO,
                container: None,
                output: VALID_OUTPUT.to_vec(),
                image_reference: None,
                created_image: None,
                created_name: None,
                create_count: 0,
                post_create_absence_count: 0,
                start_count: 0,
                remove_count: 0,
            })),
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
        let id = if state.wrong_engine || (state.wrong_engine_after_start && state.start_count > 0)
        {
            "another-engine"
        } else {
            ENGINE_ID
        };
        let info = SystemInfo {
            id: Some(id.to_owned()),
            ncpu: Some(4),
            docker_root_dir: Some("/srv/docker-data".to_owned()),
            ..Default::default()
        };
        let delay = state.info_delay;
        drop(state);
        sleep(delay).await;
        Ok(info)
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
        reference: &str,
    ) -> Result<Option<ProbeContainer>, GuestSampleFailure> {
        let mut state = self.state.lock().await;
        let container = state.container.clone();
        if state.create_count > 0
            && container.is_none()
            && state.created_name.as_deref() == Some(reference)
        {
            state.post_create_absence_count += 1;
        }
        let is_exit_lookup = state.start_count > 0
            && container
                .as_ref()
                .and_then(|item| item.name.as_deref())
                .is_some_and(|name| name.trim_start_matches('/') == reference);
        let delay = if is_exit_lookup {
            state.exit_inspect_delay
        } else {
            Duration::ZERO
        };
        let found = container.filter(|item| {
            item.id.as_deref() == Some(reference)
                || item
                    .name
                    .as_deref()
                    .is_some_and(|name| name.trim_start_matches('/') == reference)
        });
        if reference == ID && state.start_count > 0 && state.switch_engine_after_id_inspect {
            state.wrong_engine = true;
            state.switch_engine_after_id_inspect = false;
        }
        drop(state);
        sleep(delay).await;
        Ok(found)
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
        let container = ProbeContainer {
            id: Some(ID.to_owned()),
            name: Some(format!("/{name}")),
            image: request.config.image,
            labels,
            running: Some(true),
            exit_code: None,
        };
        let create_hangs = state.create_hangs;
        let loses_response = state.create_loses_response;
        let late_create_delay = state.late_create_delay;
        if create_hangs {
            let shared = Arc::clone(&self.state);
            drop(state);
            tokio::spawn(async move {
                loop {
                    let absent_lookups = shared.lock().await.post_create_absence_count;
                    if absent_lookups >= 2 {
                        break;
                    }
                    sleep(Duration::from_millis(1)).await;
                }
                sleep(late_create_delay).await;
                let mut state = shared.lock().await;
                state.late_create_after_absence_count = state.post_create_absence_count;
                state.container = Some(container);
            });
            return pending().await;
        }
        state.container = Some(container);
        drop(state);
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
        let wrong_exit_id = state.wrong_exit_id;
        if let Some(container) = &mut state.container {
            if wrong_exit_id {
                container.id = Some(OTHER_ID.to_owned());
            }
            container.running = Some(true);
            container.exit_code = None;
        }
        let start_hangs = state.start_hangs;
        let shared = Arc::clone(&self.state);
        drop(state);
        tokio::spawn(async move {
            sleep(Duration::from_millis(10)).await;
            let mut state = shared.lock().await;
            if let Some(container) = &mut state.container {
                container.running = Some(false);
                container.exit_code = Some(0);
            }
        });
        if start_hangs {
            return pending().await;
        }
        Ok(())
    }

    async fn read_output(
        &self,
        _id: &str,
        _limit: usize,
    ) -> Result<Option<Vec<u8>>, GuestSampleFailure> {
        let mut state = self.state.lock().await;
        state.output_read_count += 1;
        if state.output_hangs {
            drop(state);
            return pending().await;
        }
        if state.output_not_ready_checks > 0 {
            state.output_not_ready_checks -= 1;
            return Ok(None);
        }
        Ok(Some(state.output.clone()))
    }

    async fn remove_container(&self, id: &str) -> Result<(), GuestSampleFailure> {
        let mut state = self.state.lock().await;
        state.remove_count += 1;
        let delay = state.remove_delay;
        if state.remove_hangs {
            drop(state);
            return pending().await;
        }
        drop(state);
        sleep(delay).await;
        let mut state = self.state.lock().await;
        if state.remove_keeps_container {
            return Ok(());
        }
        if state.rename_on_remove {
            state.rename_on_remove = false;
            if let Some(container) = &mut state.container {
                container.name = Some("/renamed-guest-probe".to_owned());
            }
            return Ok(());
        }
        if state.container.as_ref().and_then(|item| item.id.as_deref()) != Some(id) {
            return Err(GuestSampleFailure::Docker);
        }
        state.container = None;
        Ok(())
    }
}

pub(super) fn image(id: &str) -> ProbeImage {
    ProbeImage {
        id: Some(id.to_owned()),
        repo_tags: vec![IMAGE_REFERENCE.to_owned()],
        os: Some("linux".to_owned()),
        architecture: Some("amd64".to_owned()),
    }
}

pub(super) fn owned_container(identity: &GuestProbeIdentity) -> ProbeContainer {
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
    identity: &'a GuestProbeIdentity,
) -> super::super::GuestSampler<'a, FakeClient> {
    super::super::GuestSampler::new(client, identity).with_timing(fast_timing())
}

pub(super) fn fast_timing() -> super::super::SamplerTiming {
    super::super::SamplerTiming {
        operation: Duration::from_millis(25),
        probe_run: Duration::from_millis(50),
        cleanup: Duration::from_millis(25),
        attempt: Duration::from_millis(400),
        cleanup_reserve: Duration::from_millis(100),
        create_reconcile: Duration::from_millis(150),
        exit_poll: Duration::from_millis(1),
    }
}

pub(super) fn unavailable(
    snapshot: super::super::GuestSampleSnapshot,
    reason: GuestSampleFailure,
) -> bool {
    snapshot.status == super::super::GuestSampleStatus::Unavailable(reason)
        && snapshot.sample == GuestResourceSample::default()
}
