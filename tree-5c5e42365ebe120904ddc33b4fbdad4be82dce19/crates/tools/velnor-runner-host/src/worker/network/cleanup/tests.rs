use std::sync::{Arc, Mutex};

use crate::HostError;
use velnor_runner_journal::journal::{OuterNetworkCleanupState, OuterNetworkRemovalProof};

use super::{
    OuterNetworkCleanupEngine, OuterNetworkCleanupLedger, OuterNetworkRemovalReceipt,
    cleanup_provisioning_network,
};
use crate::worker::WorkerNetworkPlan;

const WORKER: &str = "w0123456789abcdef0123456789abcdef";
const NETWORK_ID: &str = "c123456789abcdef";

#[derive(Clone)]
struct FakeEngine {
    state: Arc<Mutex<EngineState>>,
}

#[derive(Default)]
struct EngineState {
    network_id: Option<String>,
    fail_remove: bool,
    events: Vec<String>,
}

impl FakeEngine {
    fn new(network_id: Option<&str>) -> Self {
        Self {
            state: Arc::new(Mutex::new(EngineState {
                network_id: network_id.map(str::to_owned),
                ..EngineState::default()
            })),
        }
    }

    fn fail_remove(&self) {
        self.state.lock().expect("test mutex").fail_remove = true;
    }

    fn events(&self) -> Vec<String> {
        self.state.lock().expect("test mutex").events.clone()
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "in-memory Docker fake records deterministic effects synchronously"
)]
impl OuterNetworkCleanupEngine for FakeEngine {
    async fn inspect_owned(&self, plan: &WorkerNetworkPlan) -> Result<Option<String>, HostError> {
        let mut state = self.state.lock().expect("test mutex");
        state.events.push(format!("inspect:{}", plan.name()));
        Ok(state.network_id.clone())
    }

    async fn ensure_owned(&self, plan: &WorkerNetworkPlan) -> Result<String, HostError> {
        let mut state = self.state.lock().expect("test mutex");
        state.events.push(format!("ensure:{}", plan.name()));
        if state.network_id.is_none() {
            state.network_id = Some(NETWORK_ID.to_owned());
        }
        state.network_id.clone().ok_or(HostError::Docker)
    }

    async fn remove_owned(
        &self,
        plan: &WorkerNetworkPlan,
        network_id: &str,
    ) -> Result<(), HostError> {
        let mut state = self.state.lock().expect("test mutex");
        state
            .events
            .push(format!("remove:{}:{network_id}", plan.name()));
        if state.fail_remove {
            state.fail_remove = false;
            return Err(HostError::Docker);
        }
        if state.network_id.as_deref() != Some(network_id) {
            return Err(HostError::Identity);
        }
        state.network_id = None;
        Ok(())
    }
}

#[derive(Clone)]
struct FakeLedger {
    state: Arc<Mutex<LedgerState>>,
}

struct LedgerState {
    network: OuterNetworkCleanupState,
    events: Vec<String>,
    receipt: Option<OuterNetworkRemovalReceipt>,
}

impl FakeLedger {
    fn new(network: OuterNetworkCleanupState) -> Self {
        Self {
            state: Arc::new(Mutex::new(LedgerState {
                network,
                events: Vec::new(),
                receipt: None,
            })),
        }
    }

    fn events(&self) -> Vec<String> {
        self.state.lock().expect("test mutex").events.clone()
    }

    fn network(&self) -> OuterNetworkCleanupState {
        self.state.lock().expect("test mutex").network.clone()
    }

    fn has_receipt(&self) -> bool {
        self.state.lock().expect("test mutex").receipt.is_some()
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "in-memory journal fake records deterministic effects synchronously"
)]
impl OuterNetworkCleanupLedger for FakeLedger {
    async fn network_state(&self, launch_id: i64) -> Result<OuterNetworkCleanupState, HostError> {
        if launch_id <= 0 {
            return Err(HostError::Identity);
        }
        let mut state = self.state.lock().expect("test mutex");
        state.events.push("state".to_owned());
        Ok(state.network.clone())
    }

    async fn bind_network_id(&self, launch_id: i64, network_id: &str) -> Result<(), HostError> {
        if launch_id <= 0 {
            return Err(HostError::Identity);
        }
        let mut state = self.state.lock().expect("test mutex");
        let OuterNetworkCleanupState::NeedsReconciliation { name } = &state.network else {
            return Err(HostError::Identity);
        };
        let name = name.clone();
        state.events.push(format!("bind:{network_id}"));
        state.network = OuterNetworkCleanupState::Bound {
            name,
            id: network_id.to_owned(),
        };
        Ok(())
    }

    async fn begin_network_removal(
        &self,
        launch_id: i64,
        name: &str,
        network_id: Option<&str>,
    ) -> Result<(), HostError> {
        if launch_id <= 0 {
            return Err(HostError::Identity);
        }
        let mut state = self.state.lock().expect("test mutex");
        match &state.network {
            OuterNetworkCleanupState::Bound { name: found, id }
                if found == name && Some(id.as_str()) == network_id => {}
            OuterNetworkCleanupState::NeedsReconciliation { name: found }
                if found == name && network_id.is_none() => {}
            OuterNetworkCleanupState::RemovalPending { name: found, id }
                if found == name && id.as_deref() == network_id =>
            {
                return Ok(());
            }
            _ => return Err(HostError::Identity),
        }
        state
            .events
            .push(format!("begin:{}", network_id.unwrap_or("unbound")));
        state.network = OuterNetworkCleanupState::RemovalPending {
            name: name.to_owned(),
            id: network_id.map(str::to_owned),
        };
        Ok(())
    }

    async fn record_network_absent(
        &self,
        receipt: &OuterNetworkRemovalReceipt,
    ) -> Result<(), HostError> {
        if !receipt.network_absent || !receipt.create_effect_resolved {
            return Err(HostError::Identity);
        }
        let mut state = self.state.lock().expect("test mutex");
        let OuterNetworkCleanupState::RemovalPending { name, id } = &state.network else {
            return Err(HostError::Identity);
        };
        if receipt.launch_id != 17 || receipt.network_name != *name || receipt.network_id != *id {
            return Err(HostError::Identity);
        }
        let name = name.clone();
        let id = id.clone();
        state.events.push("record-absent".to_owned());
        state.network = OuterNetworkCleanupState::Absent { name, id };
        state.receipt = Some(receipt.clone());
        Ok(())
    }
}

#[tokio::test]
async fn uncertain_create_is_fenced_before_reconcile_and_never_releases_launch()
-> Result<(), HostError> {
    let name = format!("{WORKER}-outer");
    let engine = FakeEngine::new(None);
    let ledger =
        FakeLedger::new(OuterNetworkCleanupState::NeedsReconciliation { name: name.clone() });
    let receipt = cleanup_provisioning_network(&engine, &ledger, 17, WORKER)
        .await?
        .ok_or(HostError::Identity)?;

    assert_eq!(
        engine.events(),
        [
            format!("inspect:{name}"),
            format!("ensure:{name}"),
            format!("remove:{name}:{NETWORK_ID}")
        ]
    );
    assert_eq!(ledger.events(), ["state", "begin:unbound", "record-absent"]);
    assert_eq!(
        ledger.network(),
        OuterNetworkCleanupState::Absent { name, id: None }
    );
    assert!(receipt.network_id().is_none());
    assert!(receipt.exact_owned_labels_verified());
    assert!(receipt.create_effect_resolved());
    assert!(receipt.network_absent());
    assert!(ledger.has_receipt());
    Ok(())
}

#[tokio::test]
async fn bound_network_retries_exact_removal_without_recreating() -> Result<(), HostError> {
    let name = format!("{WORKER}-outer");
    let engine = FakeEngine::new(Some(NETWORK_ID));
    let ledger = FakeLedger::new(OuterNetworkCleanupState::Bound {
        name: name.clone(),
        id: NETWORK_ID.to_owned(),
    });
    let receipt = cleanup_provisioning_network(&engine, &ledger, 17, WORKER)
        .await?
        .ok_or(HostError::Identity)?;

    assert_eq!(engine.events(), [format!("remove:{name}:{NETWORK_ID}")]);
    assert_eq!(
        ledger.events(),
        ["state", "begin:c123456789abcdef", "record-absent"]
    );
    assert_eq!(receipt.network_id(), Some(NETWORK_ID));
    assert_eq!(
        ledger.network(),
        OuterNetworkCleanupState::Absent {
            name,
            id: Some(NETWORK_ID.to_owned())
        }
    );
    Ok(())
}

#[tokio::test]
async fn failed_remove_keeps_the_durable_pending_identity() -> Result<(), HostError> {
    let name = format!("{WORKER}-outer");
    let engine = FakeEngine::new(Some(NETWORK_ID));
    engine.fail_remove();
    let ledger = FakeLedger::new(OuterNetworkCleanupState::Bound {
        name: name.clone(),
        id: NETWORK_ID.to_owned(),
    });
    let result = cleanup_provisioning_network(&engine, &ledger, 17, WORKER).await;

    assert_eq!(result, Err(HostError::Docker));
    assert_eq!(
        ledger.network(),
        OuterNetworkCleanupState::RemovalPending {
            name,
            id: Some(NETWORK_ID.to_owned())
        }
    );
    assert!(!ledger.has_receipt());
    Ok(())
}

#[tokio::test]
async fn mismatched_or_already_absent_state_does_not_touch_docker() -> Result<(), HostError> {
    let wrong = FakeEngine::new(Some(NETWORK_ID));
    let wrong_ledger = FakeLedger::new(OuterNetworkCleanupState::Bound {
        name: "unrelated-outer".to_owned(),
        id: NETWORK_ID.to_owned(),
    });
    assert_eq!(
        cleanup_provisioning_network(&wrong, &wrong_ledger, 17, WORKER).await,
        Err(HostError::Identity)
    );
    assert_eq!(wrong.events(), Vec::<String>::new());

    let absent = FakeEngine::new(None);
    let absent_ledger = FakeLedger::new(OuterNetworkCleanupState::Absent {
        name: format!("{WORKER}-outer"),
        id: Some(NETWORK_ID.to_owned()),
    });
    assert_eq!(
        cleanup_provisioning_network(&absent, &absent_ledger, 17, WORKER).await?,
        None
    );
    assert_eq!(absent.events(), [format!("inspect:{WORKER}-outer")]);
    Ok(())
}

#[tokio::test]
async fn an_absent_journal_row_rejects_a_reappeared_owned_name() {
    let engine = FakeEngine::new(Some(NETWORK_ID));
    let ledger = FakeLedger::new(OuterNetworkCleanupState::Absent {
        name: format!("{WORKER}-outer"),
        id: None,
    });

    assert_eq!(
        cleanup_provisioning_network(&engine, &ledger, 17, WORKER).await,
        Err(HostError::Identity)
    );
    assert_eq!(engine.events(), [format!("inspect:{WORKER}-outer")]);
    assert!(!ledger.has_receipt());
}
