// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::{Value, json};
use tokio::net::UnixListener;
use tokio::sync::Notify;

use super::TestResult;
use super::seed::{CONNECTION_ID, ENGINE_ID, Pair};

#[path = "fixture/barriers.rs"]
mod barriers;
#[path = "fixture/http.rs"]
mod http;
#[path = "fixture/initializer.rs"]
mod initializer;

pub(crate) use barriers::{CreateBarrier, DeleteBarrier};

struct Container {
    name: String,
    labels: Value,
    running: bool,
    inspection: Option<Value>,
}

#[derive(Default)]
struct State {
    containers: BTreeMap<String, Container>,
    volumes: BTreeMap<String, Value>,
    networks: BTreeMap<String, (String, Value)>,
    requests: Vec<String>,
    hang_after: Option<String>,
    armed: bool,
    creates: BTreeMap<(String, String), (String, Arc<barriers::CreateGate>)>,
    deletes: BTreeMap<String, Arc<barriers::DeleteGate>>,
}

pub(crate) struct FixtureDocker {
    state: Arc<Mutex<State>>,
    hung: Arc<Notify>,
}

impl FixtureDocker {
    pub(crate) fn start(socket: &Path, pairs: &[Pair]) -> TestResult<Self> {
        let listener = UnixListener::bind(socket)?;
        let mut state = State::default();
        for pair in pairs {
            let (reservation, operation) = (
                pair.reservation.to_string(),
                (pair.reservation - 1).to_string(),
            );
            for role in ["runner", "dind"] {
                state.containers.insert(
                    pair.container_id(role),
                    Container {
                        name: format!("w{}-{role}", pair.nonce()),
                        labels: json!({"io.velnor.managed": "true", "io.velnor.reservation-id": reservation, "io.velnor.operation-id": operation, "io.velnor.connection-id": CONNECTION_ID, "io.velnor.role": role}),
                        running: true,
                        inspection: None,
                    },
                );
            }
            for (name, role) in pair.volumes() {
                state.volumes.insert(
                    name,
                    json!({"velnor.connection": CONNECTION_ID, "velnor.reservation": reservation, "velnor.operation": operation, "velnor.role": role}),
                );
            }
            state.networks.insert(
                pair.network_id(),
                (
                    format!("velnor-v{}-private", pair.nonce()),
                    json!({"io.velnor.connection-id": CONNECTION_ID, "io.velnor.engine-id": ENGINE_ID, "io.velnor.managed": "true", "io.velnor.operation-id": operation, "io.velnor.reservation-id": reservation}),
                ),
            );
        }
        let fixture = Self {
            state: Arc::new(Mutex::new(state)),
            hung: Arc::default(),
        };
        let (state, hung) = (Arc::clone(&fixture.state), Arc::clone(&fixture.hung));
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (state, hung) = (Arc::clone(&state), Arc::clone(&hung));
                tokio::spawn(async move { drop(http::serve(stream, &state, &hung).await) });
            }
        });
        Ok(fixture)
    }

    pub(crate) fn add_initializer(&self, pair: Pair) {
        lock(&self.state).containers.insert(
            pair.helper_id(),
            Container {
                name: pair.helper_name(),
                labels: json!({"velnor.role": "storage-initializer", "velnor.operation": format!("{}-1", pair.reservation - 1)}),
                running: false,
                inspection: Some(initializer::inspection(pair)),
            },
        );
    }

    pub(crate) fn delay_create(&self, kind: &str, name: &str, id: &str) -> CreateBarrier {
        let gate = Arc::new(barriers::CreateGate::default());
        lock(&self.state).creates.insert(
            (kind.to_owned(), name.to_owned()),
            (id.to_owned(), Arc::clone(&gate)),
        );
        CreateBarrier(gate)
    }

    pub(crate) fn delay_delete(&self, kind: &str, id: &str) -> DeleteBarrier {
        let gate = Arc::new(barriers::DeleteGate::default());
        lock(&self.state)
            .deletes
            .insert(format!("DELETE /{kind}/{id}"), Arc::clone(&gate));
        DeleteBarrier(gate)
    }

    pub(crate) fn remove_network(&self, pair: Pair) {
        lock(&self.state).networks.remove(&pair.network_id());
    }

    pub(crate) fn remove_volume(&self, name: &str) {
        lock(&self.state).volumes.remove(name);
    }

    pub(crate) fn relabel_initializer(&self, pair: Pair) {
        if let Some(container) = lock(&self.state).containers.get_mut(&pair.helper_id()) {
            container.labels["velnor.operation"] = json!("999-1");
        }
    }

    pub(crate) fn has_initializer(&self, pair: Pair) -> bool {
        lock(&self.state).containers.contains_key(&pair.helper_id())
    }

    pub(crate) fn hang_after(&self, request: String) {
        lock(&self.state).hang_after = Some(request);
    }

    pub(crate) async fn hung(&self) {
        self.hung.notified().await;
    }

    pub(crate) fn relabel_container(&self, id: &str) {
        if let Some(container) = lock(&self.state).containers.get_mut(id) {
            container.labels["io.velnor.reservation-id"] = json!("999");
        }
    }

    pub(crate) fn rename_container(&self, id: &str, name: &str) {
        if let Some(container) = lock(&self.state).containers.get_mut(id) {
            name.clone_into(&mut container.name);
        }
    }

    pub(crate) fn reissue_container(&self, id: &str, new_id: &str) {
        let mut state = lock(&self.state);
        if let Some(container) = state.containers.remove(id) {
            state.containers.insert(new_id.to_owned(), container);
        }
    }

    pub(crate) fn relabel_volume(&self, name: &str) {
        if let Some(labels) = lock(&self.state).volumes.get_mut(name) {
            labels["velnor.reservation"] = json!("999");
        }
    }

    pub(crate) fn has_container(&self, id: &str) -> bool {
        lock(&self.state).containers.contains_key(id)
    }

    pub(crate) fn remove_container(&self, id: &str) {
        lock(&self.state).containers.remove(id);
    }

    pub(crate) fn relabel_network(&self, pair: Pair) {
        if let Some((_, labels)) = lock(&self.state).networks.get_mut(&pair.network_id()) {
            labels["io.velnor.reservation-id"] = json!("999");
        }
    }

    pub(crate) fn set_container_running(&self, id: &str, running: bool) {
        if let Some(container) = lock(&self.state).containers.get_mut(id) {
            container.running = running;
        }
    }

    pub(crate) fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut lock(&self.state).requests)
    }

    pub(crate) fn objects(&self, pair: Pair) -> Vec<String> {
        let state = lock(&self.state);
        let containers = ["runner", "dind"]
            .map(|role| pair.container_id(role))
            .into_iter()
            .filter(|id| state.containers.contains_key(id));
        let volumes = pair
            .volumes()
            .map(|(name, _)| name)
            .into_iter()
            .filter(|name| state.volumes.contains_key(name));
        let networks = [pair.network_id()]
            .into_iter()
            .filter(|id| state.networks.contains_key(id));
        containers.chain(volumes).chain(networks).collect()
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) mod bound_neighbor {
    use super::super::{TestResult, seed};
    use crate::journal::Journal;
    use seed::Pair;

    mod binding {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/launch/jit_conflict/tests/binding.rs"
        ));
    }

    mod storage_seed {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/launch/jit_conflict/tests/storage_seed.rs"
        ));
    }

    async fn scalar(journal: &Journal, query: &str) -> TestResult<String> {
        let connection = journal.connection().await?;
        let mut rows = connection.query(query, ()).await?;
        Ok(rows.next().await?.ok_or("scalar row")?.get(0)?)
    }

    pub(crate) async fn install(
        journal: &Journal,
        pair: Pair,
        engine: &crate::launch::SelectedEngineContext,
    ) -> TestResult<()> {
        seed::bind_resources(journal, pair, engine.docker_root_digest()).await?;
        binding::install(journal, pair).await?;
        storage_seed::install(journal, pair).await?;
        seed::execute(journal, &format!("UPDATE connection_intents SET launch_phase='resources_bound',docker_id='{}',dind_id='{}' WHERE id={}", pair.container_id("runner"), pair.container_id("dind"), pair.reservation - 1)).await
    }
}
