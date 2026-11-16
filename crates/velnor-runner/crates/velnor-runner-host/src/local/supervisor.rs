// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use bollard::Docker;
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::error::HostError;

use super::authority::Client;
use super::minted::MintedAgents;
use super::pair::{ENVIRONMENT_LABEL, PAIR_LABEL};
use super::plan::{Plan, Side};
use super::policy::Policy;
use super::remove::{labelled_agents, remove_labelled};
use super::request::{LocalPairRequest, validate};

pub(super) const DECISIONS_FILE: &str = "decisions.jsonl";

#[derive(Debug, Clone)]
pub(super) struct Slot {
    pub(super) side: Side,
    pub(super) network: String,
    pub(super) label: String,
}

#[derive(Debug)]
pub(super) struct Supervisor {
    pub(super) docker: Docker,
    pub(super) client: Client,
    pub(super) plan: Plan,
    pub(super) policy: Option<Policy>,
    pub(super) decisions: PathBuf,
    pub(super) minted: MintedAgents,
    pub(super) sequence: AtomicU32,
    pub(super) pairs_removed: AtomicUsize,
    pub(super) agents_removed: AtomicUsize,
}

impl Supervisor {
    pub(super) fn request(&self, slot: &Slot, agent: &str, sequence: u32) -> LocalPairRequest {
        LocalPairRequest {
            environment: self.plan.environment.clone(),
            side: slot.side.name().to_owned(),
            agent: agent.to_owned(),
            network: slot.network.clone(),
            platform: self.plan.platform.clone(),
            runner_image: self.plan.runner_image.clone(),
            dind_image: self.plan.dind_image.clone(),
            sequence,
            runner_cpu_millicores: self.plan.limits.runner_cpu_millicores,
            runner_memory_bytes: self.plan.limits.runner_memory_bytes,
            dind_cpu_millicores: self.plan.limits.dind_cpu_millicores,
            dind_memory_bytes: self.plan.limits.dind_memory_bytes,
        }
    }

    fn slots(&self) -> Vec<Slot> {
        self.plan
            .sides
            .iter()
            .flat_map(|side| {
                (1..=side.pairs).map(|index| Slot {
                    side: side.side,
                    network: side.network.clone(),
                    label: format!("{}-{index}", side.side.name()),
                })
            })
            .collect()
    }

    fn preflight(&self) -> Result<(), HostError> {
        for slot in self.slots() {
            validate(&self.request(&slot, "preflight", 1))?;
        }
        Ok(())
    }

    async fn sweep(&self) -> Result<(), HostError> {
        let label = format!("{ENVIRONMENT_LABEL}={}", self.plan.environment);
        let mut agents = labelled_agents(&self.docker, &label).await?;
        agents.extend(self.minted.load().await?);
        agents.sort();
        agents.dedup();
        let pairs = [label.clone(), PAIR_LABEL.to_owned()];
        let removed = remove_labelled(&self.docker, &pairs).await?;
        let mut cleanup = Ok(());
        let mut unconfirmed = false;
        for (side_name, name) in agents {
            let Some(side) = Side::parse(&side_name) else {
                continue;
            };
            match self.client.delete_agent(side, &name).await {
                Ok(true) => self.minted.retired(&side_name, &name).await?,
                Ok(false) => unconfirmed = true,
                Err(_) => {
                    eprintln!("agent {name} was not deleted at runner.server");
                    cleanup = Err(HostError::Cleanup);
                }
            }
        }
        if cleanup.is_ok() && !unconfirmed {
            self.minted.clear().await?;
        }
        if removed.containers + removed.volumes > 0 {
            println!(
                "swept {} containers and {} volumes labelled {label}",
                removed.containers, removed.volumes
            );
        }
        cleanup
    }
}

pub(super) async fn stopped(stop: &mut watch::Receiver<bool>) {
    while !*stop.borrow_and_update() {
        if stop.changed().await.is_err() {
            return;
        }
    }
}

pub async fn supervise(
    docker: Docker,
    plan: Plan,
    state: &Path,
    shutdown: impl Future<Output = ()>,
) -> Result<(), HostError> {
    let client = Client::new(plan.authority, plan.token.clone());
    let policy = plan
        .policy
        .as_deref()
        .map(|name| Policy::load(state, name))
        .transpose()?;
    let supervisor = Arc::new(Supervisor {
        docker,
        client,
        plan,
        policy,
        decisions: state.join(DECISIONS_FILE),
        minted: MintedAgents::new(state),
        sequence: AtomicU32::new(0),
        pairs_removed: AtomicUsize::new(0),
        agents_removed: AtomicUsize::new(0),
    });
    supervisor.preflight()?;
    supervisor.sweep().await?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut slots = JoinSet::new();
    for slot in supervisor.slots() {
        let supervisor = Arc::clone(&supervisor);
        let stop = stop_rx.clone();
        slots.spawn(async move { supervisor.run_slot(slot, stop).await });
    }
    let mut failure = None;
    let mut shutdown = std::pin::pin!(shutdown);
    let mut stopping = false;
    loop {
        tokio::select! {
            () = &mut shutdown, if !stopping => {
                stopping = true;
                let _sent = stop_tx.send(true);
            }
            joined = slots.join_next() => match joined {
                None => break,
                Some(finished) => {
                    let outcome = finished.map_err(|_| HostError::Cleanup).and_then(|result| result);
                    if let Err(error) = outcome {
                        eprintln!("velnor-host local: a pair failed: {error}");
                        failure.get_or_insert(error);
                        let _sent = stop_tx.send(true);
                    }
                }
            },
        }
    }
    let swept = supervisor.sweep().await;
    println!(
        "{} pairs and {} agents removed",
        supervisor.pairs_removed.load(Ordering::SeqCst),
        supervisor.agents_removed.load(Ordering::SeqCst)
    );
    failure.map_or(swept, Err)
}

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;
