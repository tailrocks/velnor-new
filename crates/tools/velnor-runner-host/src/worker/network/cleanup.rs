//! Provisioning-only outer-network reconciliation and removal.

use ::bollard::Docker;

use crate::HostError;
use velnor_runner_journal::journal::{Journal, OuterNetworkCleanupState, OuterNetworkRemovalProof};

use super::WorkerNetworkPlan;

/// Docker and journal operations required to settle a provisioning-only bridge.
#[expect(
    async_fn_in_trait,
    reason = "the host uses async effect traits for deterministic failure tests"
)]
pub trait OuterNetworkCleanupEngine {
    /// Inspect a deterministic name and validate any existing network's full identity.
    async fn inspect_owned(&self, plan: &WorkerNetworkPlan) -> Result<Option<String>, HostError>;

    /// Create or reconcile the exact bridge. The caller must have persisted a
    /// create or removal intent first.
    async fn ensure_owned(&self, plan: &WorkerNetworkPlan) -> Result<String, HostError>;

    /// Remove only this exact network and verify the ID and deterministic name are absent.
    async fn remove_owned(
        &self,
        plan: &WorkerNetworkPlan,
        network_id: &str,
    ) -> Result<(), HostError>;
}

/// Durable journal operations for provisioning-only network reconciliation.
#[expect(
    async_fn_in_trait,
    reason = "the host uses async effect traits for deterministic failure tests"
)]
pub trait OuterNetworkCleanupLedger {
    /// Load the journal's network state for this launch generation.
    async fn network_state(&self, launch_id: i64) -> Result<OuterNetworkCleanupState, HostError>;

    /// Bind a validated ID after reconciling a create whose response was uncertain.
    async fn bind_network_id(&self, launch_id: i64, network_id: &str) -> Result<(), HostError>;

    /// Persist a cleanup fence before removing or settling the named resource.
    async fn begin_network_removal(
        &self,
        launch_id: i64,
        name: &str,
        network_id: Option<&str>,
    ) -> Result<(), HostError>;

    /// Persist proof that the exact generation-owned bridge is absent.
    async fn record_network_absent(
        &self,
        receipt: &OuterNetworkRemovalReceipt,
    ) -> Result<(), HostError>;
}

impl OuterNetworkCleanupLedger for Journal {
    async fn network_state(&self, launch_id: i64) -> Result<OuterNetworkCleanupState, HostError> {
        self.outer_network_cleanup_state(launch_id).await
    }

    async fn bind_network_id(&self, launch_id: i64, network_id: &str) -> Result<(), HostError> {
        self.bind_outer_network_id(launch_id, network_id).await
    }

    async fn begin_network_removal(
        &self,
        launch_id: i64,
        name: &str,
        network_id: Option<&str>,
    ) -> Result<(), HostError> {
        self.begin_outer_network_removal(launch_id, name, network_id)
            .await
    }

    async fn record_network_absent(
        &self,
        receipt: &OuterNetworkRemovalReceipt,
    ) -> Result<(), HostError> {
        self.record_outer_network_absent(receipt).await
    }
}

impl OuterNetworkCleanupEngine for Docker {
    async fn inspect_owned(&self, plan: &WorkerNetworkPlan) -> Result<Option<String>, HostError> {
        super::inspect_owned_network(self, plan).await
    }

    async fn ensure_owned(&self, plan: &WorkerNetworkPlan) -> Result<String, HostError> {
        super::ensure_worker_network(self, plan)
            .await
            .map_err(|failure| failure.error())
    }

    async fn remove_owned(
        &self,
        plan: &WorkerNetworkPlan,
        network_id: &str,
    ) -> Result<(), HostError> {
        super::remove_worker_network(self, plan, network_id).await
    }
}

/// Sealed local evidence for one provisioning-only network removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OuterNetworkRemovalReceipt {
    launch_id: i64,
    network_name: String,
    network_id: Option<String>,
    exact_owned_labels_verified: bool,
    create_effect_resolved: bool,
    network_absent: bool,
}

impl OuterNetworkRemovalProof for OuterNetworkRemovalReceipt {
    fn launch_id(&self) -> i64 {
        self.launch_id
    }

    fn network_name(&self) -> &str {
        &self.network_name
    }

    fn network_id(&self) -> Option<&str> {
        self.network_id.as_deref()
    }

    fn exact_owned_labels_verified(&self) -> bool {
        self.exact_owned_labels_verified
    }

    fn create_effect_resolved(&self) -> bool {
        self.create_effect_resolved
    }

    fn network_absent(&self) -> bool {
        self.network_absent
    }
}

/// Settle and remove a bridge left before a runner/DinD pair was bound.
///
/// If the prior create response was uncertain and no network ID was persisted,
/// the removal fence is written first; the exact deterministic create-or-inspect
/// is then reconciled and removed. This may briefly create the already-intended
/// bridge to establish an ID, but it never starts a worker. Any timeout leaves
/// the journal pending. The journal records only local network absence and does
/// not release the launch permit or resolve Acquire/JIT uncertainty.
///
/// # Errors
///
/// Returns an error for mismatched ownership, uncertain Docker responses, an
/// invalid row state, or incomplete inspect-after-remove evidence.
pub async fn cleanup_provisioning_network<
    E: OuterNetworkCleanupEngine,
    L: OuterNetworkCleanupLedger,
>(
    engine: &E,
    ledger: &L,
    launch_id: i64,
    worker_volume: &str,
) -> Result<Option<OuterNetworkRemovalReceipt>, HostError> {
    let plan = WorkerNetworkPlan::for_worker(worker_volume)?;
    let state = ledger.network_state(launch_id).await?;
    match state {
        OuterNetworkCleanupState::NotOwned => Ok(None),
        OuterNetworkCleanupState::Absent { name, .. } => {
            ensure_name(&plan, &name)?;
            if engine.inspect_owned(&plan).await?.is_some() {
                return Err(HostError::Identity);
            }
            Ok(None)
        }
        OuterNetworkCleanupState::NeedsReconciliation { name } => {
            ensure_name(&plan, &name)?;
            if let Some(network_id) = engine.inspect_owned(&plan).await? {
                ledger.bind_network_id(launch_id, &network_id).await?;
                remove_with_id(engine, ledger, launch_id, &plan, network_id).await
            } else {
                remove_without_durable_id(engine, ledger, launch_id, &plan).await
            }
        }
        OuterNetworkCleanupState::Bound { name, id }
        | OuterNetworkCleanupState::RemovalPending { name, id: Some(id) } => {
            ensure_name(&plan, &name)?;
            remove_with_id(engine, ledger, launch_id, &plan, id).await
        }
        OuterNetworkCleanupState::RemovalPending { name, id: None } => {
            ensure_name(&plan, &name)?;
            remove_without_durable_id(engine, ledger, launch_id, &plan).await
        }
    }
}

async fn remove_with_id<E: OuterNetworkCleanupEngine, L: OuterNetworkCleanupLedger>(
    engine: &E,
    ledger: &L,
    launch_id: i64,
    plan: &WorkerNetworkPlan,
    network_id: String,
) -> Result<Option<OuterNetworkRemovalReceipt>, HostError> {
    ledger
        .begin_network_removal(launch_id, plan.name(), Some(&network_id))
        .await?;
    engine.remove_owned(plan, &network_id).await?;
    let receipt = OuterNetworkRemovalReceipt {
        launch_id,
        network_name: plan.name().to_owned(),
        network_id: Some(network_id),
        exact_owned_labels_verified: true,
        create_effect_resolved: true,
        network_absent: true,
    };
    ledger.record_network_absent(&receipt).await?;
    Ok(Some(receipt))
}

async fn remove_without_durable_id<E: OuterNetworkCleanupEngine, L: OuterNetworkCleanupLedger>(
    engine: &E,
    ledger: &L,
    launch_id: i64,
    plan: &WorkerNetworkPlan,
) -> Result<Option<OuterNetworkRemovalReceipt>, HostError> {
    ledger
        .begin_network_removal(launch_id, plan.name(), None)
        .await?;
    let network_id = engine.ensure_owned(plan).await?;
    engine.remove_owned(plan, &network_id).await?;
    let receipt = OuterNetworkRemovalReceipt {
        launch_id,
        network_name: plan.name().to_owned(),
        network_id: None,
        exact_owned_labels_verified: true,
        create_effect_resolved: true,
        network_absent: true,
    };
    ledger.record_network_absent(&receipt).await?;
    Ok(Some(receipt))
}

fn ensure_name(plan: &WorkerNetworkPlan, name: &str) -> Result<(), HostError> {
    if plan.name() == name {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

#[cfg(test)]
mod tests;
