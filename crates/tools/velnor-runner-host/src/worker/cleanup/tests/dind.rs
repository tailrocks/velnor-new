use tempfile::TempDir;

use crate::HostError;
use crate::worker::cleanup::*;

use super::fixtures::{FakeEngine, FakeLedger, diagnostics_store, identity};

#[tokio::test]
async fn uncertain_dind_stop_is_reconciled_before_any_outer_removal() -> Result<(), String> {
    let engine = FakeEngine::running_with_lost_dind_stop_response();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let generation = identity(None)?;

    let interrupted = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation.clone(),
        PostActionDisposition::Interrupted {
            reason_class: "operator_deadline".to_owned(),
        },
        RunnerStopPolicy::RequireStopped,
    )
    .await;
    assert_eq!(interrupted, Err(HostError::Docker));
    assert!(!engine.is_absent(OuterContainerRole::Runner));
    assert!(!engine.is_absent(OuterContainerRole::Dind));
    assert!(engine.events().contains(&"stop-dind".to_owned()));
    assert!(!ledger.has_step("after-DindTermination"));
    assert!(!ledger.completed());
    assert!(!engine.events().iter().any(|event| matches!(
        event.as_str(),
        "remove-runner" | "remove-dind" | "remove-volumes" | "remove-outer-network"
    )));

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation,
        PostActionDisposition::Interrupted {
            reason_class: "operator_deadline".to_owned(),
        },
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(proof.dind_stopped());
    assert!(proof.physical_capacity_reclaimable());
    assert!(ledger.has_step("after-DindTermination"));
    assert!(ledger.completed());
    Ok(())
}
