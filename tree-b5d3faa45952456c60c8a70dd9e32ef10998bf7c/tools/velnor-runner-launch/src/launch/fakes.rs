//! Fake pair engine and launch seed helpers for launch tests.
//!
//! Every item is `cfg(test)`: production builds see an empty module. The
//! canonical suite form bans `cfg(test)` module declarations, so shared
//! launch scaffolding lives here instead of a test-only module.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
use super::harness::{Mode, Script};
#[cfg(test)]
use velnor_runner_host::stage::PairEngine;
#[cfg(test)]
use velnor_runner_host::worker::CreateProjection;
#[cfg(test)]
use velnor_runner_host::{HostError, Journal, Outcome};

#[cfg(test)]
pub(crate) fn hex(n: u64) -> String {
    format!("{n:064x}")
}

#[cfg(test)]
pub(crate) fn valid_worker_volume(volume: &str) -> bool {
    volume.starts_with('w')
        && volume.len() == 33
        && volume.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
pub(crate) struct Engine {
    ids: Mutex<Vec<String>>,
    running: Mutex<HashMap<String, bool>>,
    names: Mutex<HashMap<String, String>>,
    owners: Mutex<HashMap<String, (String, String)>>,
    volumes: Mutex<HashMap<String, (String, String)>>,
    removed: Mutex<Vec<String>>,
    removed_volumes: Mutex<Vec<String>>,
    next: Mutex<u64>,
}

#[cfg(test)]
impl Engine {
    pub(crate) fn new() -> Self {
        Self {
            ids: Mutex::new(Vec::new()),
            running: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            owners: Mutex::new(HashMap::new()),
            volumes: Mutex::new(HashMap::new()),
            removed: Mutex::new(Vec::new()),
            removed_volumes: Mutex::new(Vec::new()),
            next: Mutex::new(1),
        }
    }

    pub(crate) fn plant(&self, id: &str, up: bool) -> Result<(), HostError> {
        self.ids
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(id.to_owned());
        self.running
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(id.to_owned(), up);
        Ok(())
    }

    pub(crate) fn plant_owned(
        &self,
        worker: &str,
        role: &str,
        id: &str,
        up: bool,
    ) -> Result<(), HostError> {
        self.plant(id, up)?;
        let name = format!("{worker}-{role}");
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(name.clone(), id.to_owned());
        self.owners
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(name, (worker.to_owned(), role.to_owned()));
        Ok(())
    }

    pub(crate) fn plant_worker_volumes(&self, worker: &str) -> Result<(), HostError> {
        let mut volumes = self.volumes.lock().map_err(|_| HostError::Docker)?;
        for (name, role) in volume_roles(worker) {
            volumes.insert(name, (worker.to_owned(), role));
        }
        Ok(())
    }

    pub(crate) fn foreign_volume(&self, name: &str) -> Result<(), HostError> {
        self.volumes.lock().map_err(|_| HostError::Docker)?.insert(
            name.to_owned(),
            ("foreign".to_owned(), "foreign".to_owned()),
        );
        Ok(())
    }

    pub(crate) fn foreign(&self, id: &str) -> Result<(), HostError> {
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(id.to_owned(), id.to_owned());
        self.plant(id, true)
    }

    pub(crate) fn foreign_named_worker(
        &self,
        worker: &str,
        role: &str,
        id: &str,
        up: bool,
    ) -> Result<(), HostError> {
        self.plant(id, up)?;
        let name = format!("{worker}-{role}");
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(name.clone(), id.to_owned());
        self.owners
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(name, ("foreign-worker".to_owned(), role.to_owned()));
        Ok(())
    }

    pub(crate) fn removed(&self) -> Result<Vec<String>, HostError> {
        self.removed
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| HostError::Docker)
    }

    pub(crate) fn removed_volumes(&self) -> Result<Vec<String>, HostError> {
        self.removed_volumes
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| HostError::Docker)
    }

    pub(crate) fn alive(&self, id: &str) -> Result<bool, HostError> {
        Ok(self
            .ids
            .lock()
            .map_err(|_| HostError::Docker)?
            .iter()
            .any(|kept| kept == id))
    }

    pub(crate) fn set_running(&self, id: &str, running: bool) -> Result<(), HostError> {
        self.running
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(id.to_owned(), running);
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::unused_async_trait_impl,
    reason = "fake engine matches the async trait without I/O"
)]
impl PairEngine for Engine {
    async fn prepare_volumes(&self, volume: &str) -> Result<(), HostError> {
        self.plant_worker_volumes(volume)
    }

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        let mut next = self.next.lock().map_err(|_| HostError::Docker)?;
        let id = hex(*next);
        *next = next.saturating_add(1);
        drop(next);
        self.plant(&id, false)?;
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(spec.name.clone(), id.clone());
        let labels = parse_labels(&spec.labels);
        self.owners
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(spec.name.clone(), labels);
        Ok(id)
    }

    async fn start(&self, id: &str) -> Result<(), HostError> {
        self.set_running(id, true)
    }

    async fn write_jit(&self, _id: &str, _jit: &[u8]) -> Result<(), HostError> {
        Ok(())
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        self.removed
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(id.to_owned());
        self.ids
            .lock()
            .map_err(|_| HostError::Docker)?
            .retain(|kept| kept != id);
        self.running
            .lock()
            .map_err(|_| HostError::Docker)?
            .remove(id);
        let mut names = self.names.lock().map_err(|_| HostError::Docker)?;
        let name = names
            .iter()
            .find(|(_, found)| found.as_str() == id)
            .map(|(name, _)| name.clone());
        if let Some(name) = name {
            names.remove(&name);
            self.owners
                .lock()
                .map_err(|_| HostError::Docker)?
                .remove(&name);
        }
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
        volume: &str,
        role: &str,
    ) -> Result<Option<String>, HostError> {
        let names = self.names.lock().map_err(|_| HostError::Docker)?;
        let Some(id) = names.get(name) else {
            return Ok(None);
        };
        let owners = self.owners.lock().map_err(|_| HostError::Docker)?;
        let Some((owner, found_role)) = owners.get(name) else {
            return Err(HostError::Docker);
        };
        if owner != volume || found_role != role {
            return Err(HostError::Docker);
        }
        Ok(Some(id.clone()))
    }

    async fn remove_worker_volumes(&self, worker: &str) -> Result<bool, HostError> {
        let expected = volume_roles(worker);
        let mut volumes = self.volumes.lock().map_err(|_| HostError::Docker)?;
        for (name, role) in &expected {
            if let Some((owner, found_role)) = volumes.get(name)
                && (owner != worker || found_role != role)
            {
                return Ok(false);
            }
        }
        for (name, _) in expected {
            if volumes.remove(&name).is_some() {
                self.removed_volumes
                    .lock()
                    .map_err(|_| HostError::Docker)?
                    .push(name);
            }
        }
        Ok(true)
    }

    async fn running(&self, id: &str) -> Result<bool, HostError> {
        Ok(self
            .running
            .lock()
            .map_err(|_| HostError::Docker)?
            .get(id)
            .copied()
            .unwrap_or(false))
    }
}

#[cfg(test)]
pub(crate) async fn seed_done(
    journal: &Journal,
    subject: &str,
    runner: &str,
    dind: &str,
) -> Result<String, String> {
    let id = journal
        .begin("launch", subject)
        .await
        .map_err(|err| err.to_string())?;
    let volume = format!("w{id}");
    journal
        .bind_worker_volume(id, &volume)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker(id, Some(runner), Some(dind))
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Done)
        .await
        .map_err(|err| err.to_string())?;
    Ok(volume)
}

#[cfg(test)]
pub(crate) fn script() -> Script {
    Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    }
}

#[cfg(test)]
fn parse_labels(labels: &[String]) -> (String, String) {
    let worker = labels
        .iter()
        .find_map(|label| label.strip_prefix("velnor.worker="))
        .unwrap_or_default()
        .to_owned();
    let role = labels
        .iter()
        .find_map(|label| label.strip_prefix("velnor.role="))
        .unwrap_or_default()
        .to_owned();
    (worker, role)
}

#[cfg(test)]
fn volume_roles(worker: &str) -> [(String, String); 3] {
    [
        (worker.to_owned(), "socket".to_owned()),
        (format!("{worker}-work"), "work".to_owned()),
        (format!("{worker}-docker"), "dind-data".to_owned()),
    ]
}
