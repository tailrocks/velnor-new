//! Verified stage stops and recorded deletes drive `drive` and `remove_recorded`.

use std::collections::HashMap;
use std::sync::Mutex;

use super::HostError;
use super::launch::{SelectedEngineContext, VerifiedWorkerImages};
use super::stage::{
    Forget, PairEngine, PairSink, PairStop, PartialPair, StageAdmission, decide, drive,
};
use super::worker::guest_image::test_support::TestGuestImage;
use super::worker::{
    CreateProjection, ResourceBudget, WorkerVolumeRole, WorkerVolumeVerification,
    test_resource_budget,
};

mod verified;

// join_dind_net accepts only a 64-hex container id.
const FIRST_CONTAINER_ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";

struct Fake {
    events: Mutex<Vec<&'static str>>,
    ids: Mutex<Vec<String>>,
    names: Mutex<HashMap<String, String>>,
    removed: Mutex<Vec<String>>,
    creates: Mutex<Vec<CreateProjection>>,
    budgets: Mutex<Vec<ResourceBudget>>,
    /// Fail the Nth call of this method name. `None` never fails.
    fail_at: Mutex<Option<(&'static str, u8)>>,
    counts: Mutex<HashMap<&'static str, u8>>,
}

impl Fake {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            ids: Mutex::new(Vec::new()),
            names: Mutex::new(HashMap::new()),
            removed: Mutex::new(Vec::new()),
            creates: Mutex::new(Vec::new()),
            budgets: Mutex::new(Vec::new()),
            fail_at: Mutex::new(None),
            counts: Mutex::new(HashMap::new()),
        }
    }

    fn events(&self) -> Vec<&'static str> {
        self.events
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn removed(&self) -> Vec<String> {
        self.removed
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn budgets(&self) -> Vec<ResourceBudget> {
        self.budgets
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn creates(&self) -> Vec<CreateProjection> {
        self.creates
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn hit(&self, event: &'static str) -> Result<(), HostError> {
        let mut counts = self.counts.lock().map_err(|_| HostError::Docker)?;
        let seen = counts.entry(event).or_insert(0);
        *seen = seen.saturating_add(1);
        let nth = *seen;
        drop(counts);
        let fail = self.fail_at.lock().map_err(|_| HostError::Docker)?;
        if let Some((name, at)) = *fail
            && name == event
            && nth == at
        {
            return Err(HostError::Docker);
        }
        Ok(())
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "fake engine matches the async trait and does not await"
)]
impl PairEngine for Fake {
    async fn verify_worker_images(
        &self,
        expected: &SelectedEngineContext,
        images: &VerifiedWorkerImages,
    ) -> Result<(), HostError> {
        if expected.matches_images(images) {
            Ok(())
        } else {
            Err(HostError::Ownership)
        }
    }

    async fn prepare_volumes(
        &self,
        _volume: &str,
        _admission: &StageAdmission,
        _expected: &SelectedEngineContext,
        _images: &VerifiedWorkerImages,
    ) -> Result<(), HostError> {
        push(&self.events, "volumes")
    }

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        self.hit("create")?;
        self.creates
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(spec.clone());
        self.budgets
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(spec.resource_budget);
        push(&self.events, "create")?;
        let mut ids = self.ids.lock().map_err(|_| HostError::Docker)?;
        let id = format!("{:064x}", ids.len() + 1);
        ids.push(id.clone());
        drop(ids);
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(spec.name.clone(), id.clone());
        Ok(id)
    }

    async fn start(&self, _id: &str) -> Result<(), HostError> {
        self.hit("start")?;
        push(&self.events, "start")
    }

    async fn write_jit(&self, _id: &str, _jit: &[u8]) -> Result<(), HostError> {
        push(&self.events, "jit")
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        push(&self.events, "remove")?;
        self.removed
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(id.to_owned());
        let mut ids = self.ids.lock().map_err(|_| HostError::Docker)?;
        ids.retain(|kept| kept != id);
        Ok(())
    }

    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError> {
        let ids = self.ids.lock().map_err(|_| HostError::Docker)?;
        if ids.iter().any(|id| id == name) {
            return Ok(Some(name.to_owned()));
        }
        let names = self.names.lock().map_err(|_| HostError::Docker)?;
        Ok(names.get(name).cloned())
    }

    async fn worker_id_for_name(
        &self,
        name: &str,
        _volume: &str,
        _role: &str,
    ) -> Result<Option<String>, HostError> {
        let names = self.names.lock().map_err(|_| HostError::Docker)?;
        Ok(names.get(name).cloned())
    }

    async fn verify_volume(
        &self,
        _worker: &str,
        _role: WorkerVolumeRole,
    ) -> Result<WorkerVolumeVerification, HostError> {
        Ok(WorkerVolumeVerification::Absent)
    }

    async fn remove_worker_volumes(&self, _volume: &str) -> Result<bool, HostError> {
        Ok(true)
    }

    async fn running(&self, _id: &str) -> Result<bool, HostError> {
        Ok(false)
    }
}

fn push(events: &Mutex<Vec<&'static str>>, event: &'static str) -> Result<(), HostError> {
    events.lock().map_err(|_| HostError::Docker)?.push(event);
    Ok(())
}

async fn drive_test<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    resource_budget: ResourceBudget,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
) -> Result<PartialPair, HostError> {
    let input = TestGuestImage::new()?;
    drive(
        engine,
        private_volume,
        resource_budget,
        jit,
        stop,
        &input.connection,
        &input.images,
        &input.selected_engine,
        &input.file,
        StageAdmission::test_fixture_for(&input.selected_engine),
        sink,
    )
    .await
}

mod stops;
