use tempfile::TempDir;

use crate::worker::cleanup::*;

use super::fixtures::{
    FakeEngine, FakeLedger, OUTER_NETWORK_ID, WORKER, diagnostics_store,
    identity_with_outer_network, index,
};

#[tokio::test]
async fn linux_bridge_is_removed_after_containers_and_before_volumes() -> Result<(), String> {
    let engine = FakeEngine::running_with_outer_network();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity_with_outer_network(None)?,
        PostActionDisposition::Completed,
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;
    let journal_proof = &proof as &dyn velnor_runner_journal::journal::PhysicalCleanupProof;

    assert_eq!(
        journal_proof.outer_network_name(),
        Some("w0123456789abcdef0123456789abcdef-outer")
    );
    assert_eq!(journal_proof.outer_network_id(), Some(OUTER_NETWORK_ID));
    assert!(journal_proof.outer_network_absent());
    assert!(engine.outer_network_is_absent());
    let events = engine.events();
    let runner = index(&events, "remove-runner");
    let dind = index(&events, "remove-dind");
    let network = index(&events, "remove-outer-network");
    let volumes = index(&events, "remove-volumes");
    assert!(runner < dind && dind < network && network < volumes);
    assert!(ledger.has_step("before-OuterNetworkRemoval"));
    assert!(ledger.has_step("after-OuterNetworkRemoval"));
    Ok(())
}

#[tokio::test]
async fn failed_bridge_removal_retries_exact_identity_without_premature_cleanup()
-> Result<(), String> {
    let engine = FakeEngine::running_with_network_removal_failure();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let identity = identity_with_outer_network(None)?;

    let first = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity.clone(),
        PostActionDisposition::Unknown,
        RunnerStopPolicy::StopAtDeadline {
            grace_seconds: 3,
            reason_class: "operator_deadline".to_owned(),
        },
    )
    .await;
    assert_eq!(first, Err(crate::HostError::Docker));
    assert!(!ledger.completed());
    assert!(!engine.outer_network_is_absent());
    assert!(
        !engine
            .events()
            .iter()
            .any(|event| event == "remove-volumes")
    );

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::StopAtDeadline {
            grace_seconds: 3,
            reason_class: "operator_deadline".to_owned(),
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    assert!(ledger.completed());
    assert!(engine.outer_network_is_absent());
    let (network_name, network_id) = proof
        .outer_network()
        .ok_or_else(|| "outer network identity was not retained".to_owned())?;
    assert_eq!(network_name, format!("{WORKER}-outer"));
    assert_eq!(network_id, OUTER_NETWORK_ID);
    assert_eq!(
        proof.cleanup_disposition(),
        CleanupDisposition::Interrupted {
            reason_class: "unknown_post_actions"
        }
    );
    Ok(())
}
