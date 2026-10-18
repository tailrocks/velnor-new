use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use velnor_runner_github::Method;

use super::support::{
    FakeLane, Fixture, REQUEST_ID, SET_ID, exchange, fixture_storage, selected_engine, test_images,
};
use crate::launch::resource_probe::StartPermit;
use crate::launch::steps::scoped::{acquire, runner};
use crate::worker::{self, test_resource_budget};

#[path = "docker_pair/server.rs"]
mod server;

use server::{CapturedRequest, DIND_ID, PairDockerApi, RUNNER_ID, route};

#[tokio::test]
async fn scoped_start_uses_bollard_to_create_and_start_the_verified_pair() -> Result<(), String> {
    let setup = PairSetup::open().await?;
    let result = setup.run().await?;
    assert_eq!(result.started.dind_id, DIND_ID);
    assert_eq!(result.started.runner_id, RUNNER_ID);
    assert_eq!(result.methods, [Method::Post, Method::Post]);
    assert_eq!(result.jit_input, b"opaque-jit");
    let creates: Vec<_> = result
        .requests
        .iter()
        .filter(|request| route(request).ends_with("/containers/create"))
        .collect();
    assert_eq!(creates.len(), 2);
    verify_container_projections(&creates, &result.images, &result.state_root)?;
    assert_eq!(count_route(&result.requests, "/volumes/create"), 3);
    assert_eq!(count_route(&result.requests, "/start"), 2);
    assert_mutations_follow_fresh_checks(&result.requests)?;
    assert_eq!(result.occupied_reservations, 1);
    Ok(())
}

struct PairSetup {
    fixture: Fixture,
    images: crate::launch::VerifiedWorkerImages,
    selected: crate::launch::SelectedEngineContext,
    docker_api: PairDockerApi,
    state: PrivateState,
    guest_image: crate::worker::guest_image::GuestImageFile,
    physical: runner::PhysicalIntent,
    lane: FakeLane,
}

struct PairResult {
    started: crate::worker::Started,
    methods: Vec<Method>,
    requests: Vec<CapturedRequest>,
    jit_input: Vec<u8>,
    images: crate::launch::VerifiedWorkerImages,
    state_root: PathBuf,
    occupied_reservations: u64,
}

impl PairSetup {
    async fn open() -> Result<Self, String> {
        let fixture = Fixture::open("scoped-bollard-pair").await?;
        let permit = StartPermit::test_fixture();
        let storage = fixture_storage(&permit)?;
        let images = test_images(permit.docker_root_digest());
        let selected = selected_engine(&images)?;
        let docker_api = PairDockerApi::open(images.clone())?;
        let state = PrivateState::new()?;
        let guest_image =
            worker::guest_image::write_guest_image(&state.path, &fixture.binding, &images)
                .map_err(|error| error.to_string())?;
        let physical = runner::reserve(&fixture.scope, permit, storage)
            .await
            .map_err(|error| error.to_string())?;
        let mut lane = fake_lane(&physical.runner_name);
        assert_eq!(
            acquire::run(&mut lane, "queue-token", &fixture.scope, &fixture.request)
                .await
                .map_err(|error| error.to_string())?,
            acquire::AcquireDisposition::Acquired
        );
        Ok(Self {
            fixture,
            images,
            selected,
            docker_api,
            state,
            guest_image,
            physical,
            lane,
        })
    }

    async fn run(self) -> Result<PairResult, String> {
        let Self {
            fixture,
            images,
            selected,
            docker_api,
            state,
            guest_image,
            physical,
            mut lane,
        } = self;
        let budget = test_resource_budget().map_err(|error| error.to_string())?;
        let docker = docker_api.docker.clone();
        let connection = &fixture.binding;
        let expected_images = &images;
        let expected_engine = &selected;
        let expected_guest = &guest_image;
        let start_error = Arc::new(Mutex::new(None));
        let callback_error = Arc::clone(&start_error);
        let started = runner::start(
            &mut lane,
            "admin-token",
            &fixture.scope,
            &physical,
            &docker_api.docker,
            &selected,
            &images,
            move |volume, jit, bind, admission| {
                let volume = volume.to_owned();
                let jit = jit.to_vec();
                async move {
                    let result = crate::launch::bind::start_bound(
                        &docker,
                        &volume,
                        &jit,
                        budget,
                        connection,
                        expected_images,
                        expected_engine,
                        expected_guest,
                        admission,
                        &bind,
                    )
                    .await;
                    if let Err(error) = &result {
                        if let Ok(mut slot) = callback_error.lock() {
                            *slot = Some(format!("{error:?}"));
                        }
                    }
                    result
                }
            },
        )
        .await;
        let started = match started {
            Ok(started) => started,
            Err(error) => {
                let detail = start_error
                    .lock()
                    .ok()
                    .and_then(|slot| slot.clone())
                    .unwrap_or_else(|| "no callback error".to_owned());
                let intent = fixture
                    .scope
                    .intent(physical.intent_id)
                    .await
                    .map(|row| format!("{row:?}"))
                    .unwrap_or_else(|journal_error| format!("intent read failed: {journal_error}"));
                return Err(format!(
                    "{error} after {} GitHub calls; stage error: {detail}; intent: {intent}",
                    lane.methods.len(),
                ));
            }
        };
        let methods = lane.methods;
        let api = docker_api.finish().await?;
        let occupied_reservations = fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?;
        Ok(PairResult {
            started,
            methods,
            requests: api.requests,
            jit_input: api.jit_input,
            images,
            state_root: state.path.clone(),
            occupied_reservations,
        })
    }
}

fn fake_lane(runner_name: &str) -> FakeLane {
    FakeLane::new([
        Ok(exchange(
            200,
            &format!(r#"{{"count":1,"value":[{REQUEST_ID}]}}"#),
        )),
        Ok(exchange(
            200,
            &serde_json::json!({
                "encodedJITConfig": "opaque-jit",
                "runner": {
                    "id": 9001,
                    "name": runner_name,
                    "runnerScaleSetId": SET_ID
                }
            })
            .to_string(),
        )),
    ])
}

fn verify_container_projections(
    requests: &[&CapturedRequest],
    images: &crate::launch::VerifiedWorkerImages,
    state_root: &std::path::Path,
) -> Result<(), String> {
    let dind: Value =
        serde_json::from_slice(&requests[0].body).map_err(|error| error.to_string())?;
    let runner: Value =
        serde_json::from_slice(&requests[1].body).map_err(|error| error.to_string())?;
    assert_eq!(dind["Image"], images.dind().runtime_id());
    assert_eq!(dind["HostConfig"]["Privileged"], true);
    assert_eq!(runner["Image"], images.runner().runtime_id());
    assert_eq!(runner["HostConfig"]["Privileged"], false);
    assert_eq!(
        runner["HostConfig"]["NetworkMode"],
        format!("container:{DIND_ID}")
    );
    let mounts = runner["HostConfig"]["Mounts"]
        .as_array()
        .ok_or_else(|| "Runner mounts are absent".to_owned())?;
    let guest_mount = mounts
        .iter()
        .find(|mount| mount["Target"] == "/opt/velnor/guest-image.json")
        .ok_or_else(|| "Guest image mount is absent".to_owned())?;
    let source = guest_mount["Source"]
        .as_str()
        .ok_or_else(|| "Guest image source is absent".to_owned())?;
    assert!(source.starts_with(state_root.to_string_lossy().as_ref()));
    assert_eq!(guest_mount["ReadOnly"], true);
    assert!(
        requests
            .iter()
            .all(|request| { !String::from_utf8_lossy(&request.body).contains("opaque-jit") })
    );
    Ok(())
}

fn assert_mutations_follow_fresh_checks(requests: &[CapturedRequest]) -> Result<(), String> {
    for (index, request) in requests.iter().enumerate() {
        if !is_mutation(request) {
            continue;
        }
        let previous = requests
            .get(index.saturating_sub(4)..index)
            .ok_or_else(|| "Docker mutation has no preceding identity check".to_owned())?;
        let routes: Vec<_> = previous.iter().map(route).collect();
        if routes.len() != 4
            || !routes[0].ends_with("/info")
            || !routes[1].contains("/images/")
            || !routes[2].contains("/images/")
            || !routes[3].ends_with("/info")
        {
            return Err("Docker mutation did not follow fresh engine and image checks".to_owned());
        }
    }
    Ok(())
}

fn is_mutation(request: &CapturedRequest) -> bool {
    let route = route(request);
    request.method == "POST"
        && (route.ends_with("/volumes/create")
            || route.ends_with("/containers/create")
            || route.ends_with("/start")
            || route.ends_with("/attach"))
}

fn count_route(requests: &[CapturedRequest], suffix: &str) -> usize {
    requests
        .iter()
        .filter(|request| route(request).ends_with(suffix))
        .count()
}

struct PrivateState {
    path: PathBuf,
}

impl PrivateState {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("velnor-pair-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).map_err(|error| error.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        Ok(Self { path })
    }
}

impl Drop for PrivateState {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.path);
    }
}
