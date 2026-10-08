use tempfile::TempDir;

use crate::HostError;
use crate::worker::cleanup::*;

use super::fixtures::{
    CHILD_ID, FakeEngine, FakeLedger, NETWORK_ID, diagnostics_store, identity, index,
};

#[tokio::test]
async fn cleanup_stops_dind_after_children_and_before_outer_removal() -> Result<(), String> {
    let engine = FakeEngine::running();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::Completed,
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;

    assert!(proof.launch_fenced());
    assert!(proof.all_owned_children_networks_and_volumes_absent());
    assert_eq!(proof.cleanup_disposition(), CleanupDisposition::Completed);
    assert!(proof.post_actions_completed());
    assert_eq!(proof.children().container_ids(), &[CHILD_ID.to_owned()]);
    assert_eq!(proof.children().network_ids(), &[NETWORK_ID.to_owned()]);
    assert!(proof.dind_stopped());
    assert_eq!(proof.absent_containers().len(), 2);
    assert_eq!(proof.absent_volumes().len(), 6);
    let events = engine.events();
    let diagnostic = index(&events, "diagnostics");
    let child = index(&events, "remove-child");
    let child_empty = index(&events, "children-empty");
    let dind_stop = index(&events, "stop-dind");
    let runner = index(&events, "remove-runner");
    let dind = index(&events, "remove-dind");
    let volumes = index(&events, "remove-volumes");
    assert!(diagnostic < child && child < child_empty && child_empty < dind_stop);
    assert!(dind_stop < runner && runner < dind && dind < volumes);
    assert!(ledger.has_step("before-DindTermination"));
    assert!(ledger.has_step("after-DindTermination"));
    assert!(ledger.completed());
    Ok(())
}

#[tokio::test]
async fn journal_proof_preserves_optional_and_distinct_observed_ids() -> Result<(), String> {
    let engine = FakeEngine::running();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let observed = ObservedJobIdentity::new(
        73,
        Some(4),
        "opaque-scale-set-job-901".to_owned(),
        Some(8_765),
        29,
        "velnor-17".to_owned(),
    )
    .map_err(|error| error.to_string())?;

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(Some(observed))?,
        PostActionDisposition::Completed,
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;
    let journal_proof = &proof as &dyn velnor_runner_journal::journal::PhysicalCleanupProof;

    assert_eq!(journal_proof.observed_workflow_run_id(), Some(73));
    assert_eq!(journal_proof.observed_attempt(), Some(4));
    assert_eq!(
        journal_proof.observed_job_id(),
        Some("opaque-scale-set-job-901")
    );
    assert_eq!(journal_proof.observed_actions_job_id(), Some(8_765));
    assert_eq!(journal_proof.observed_runner_id(), Some(29));
    assert_eq!(journal_proof.observed_runner_name(), Some("velnor-17"));
    assert_eq!(journal_proof.outer_network_name(), None);
    assert_eq!(journal_proof.outer_network_id(), None);
    assert!(journal_proof.outer_network_absent());
    assert!(journal_proof.launch_fenced());
    assert!(journal_proof.all_owned_children_networks_and_volumes_absent());
    assert_eq!(
        journal_proof.post_actions(),
        velnor_runner_journal::journal::PostActionDisposition::Completed
    );
    assert_eq!(
        journal_proof.cleanup_disposition(),
        velnor_runner_journal::journal::CleanupDisposition::Completed
    );
    Ok(())
}

#[tokio::test]
async fn diagnostic_retention_failure_prevents_resource_deletion() -> Result<(), String> {
    let engine = FakeEngine::running_with_diagnostics(None);
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let result = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await;

    assert_eq!(result, Err(HostError::Docker));
    let events = engine.events();
    assert!(!events.iter().any(|event| event.starts_with("remove-")));
    assert!(!ledger.completed());
    Ok(())
}

#[tokio::test]
async fn missing_runner_and_missing_receipt_cannot_fabricate_retained_logs() -> Result<(), String> {
    let engine = FakeEngine::absent_runner_without_diagnostics();
    let ledger = FakeLedger::default();
    ledger.set_runner_start_observation(
        velnor_runner_journal::journal::RunnerStartObservation::MayHaveStarted,
    );
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let result = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await;

    assert_eq!(result, Err(HostError::Docker));
    let events = engine.events();
    assert!(events.contains(&"diagnostics".to_owned()));
    assert!(!events.iter().any(|event| event.starts_with("remove-")));
    assert!(!ledger.completed());
    Ok(())
}

#[tokio::test]
async fn physical_cleanup_with_unknown_post_actions_does_not_claim_success() -> Result<(), String> {
    let engine = FakeEngine::running();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::StopAtDeadline {
            grace_seconds: 3,
            reason_class: "operator_deadline".to_owned(),
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    assert!(proof.physical_capacity_reclaimable());
    assert!(!proof.post_actions_completed());
    assert_eq!(proof.post_actions(), &PostActionDisposition::Unknown);
    Ok(())
}

#[tokio::test]
async fn already_drained_checkpoint_recovers_after_dind_was_removed() -> Result<(), String> {
    let engine = FakeEngine::running_with_volume_failure();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let generation = identity(None)?;

    let failed = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation.clone(),
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await;
    assert_eq!(failed, Err(HostError::Docker));
    assert!(ledger.has_drained_checkpoint());
    assert!(engine.is_absent(OuterContainerRole::Dind));
    assert!(engine.is_absent(OuterContainerRole::Runner));

    engine.allow_volume_cleanup();
    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(!proof.post_actions_completed());
    assert!(engine.is_absent(OuterContainerRole::Runner));
    assert!(engine.is_absent(OuterContainerRole::Dind));
    Ok(())
}

#[tokio::test]
async fn lost_child_remove_response_reconciles_before_outer_cleanup() -> Result<(), String> {
    let engine = FakeEngine::running_with_lost_child_response();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let generation = identity(None)?;

    let first = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation.clone(),
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await;
    assert_eq!(first, Err(HostError::Docker));
    assert!(
        engine
            .events()
            .contains(&"remove-child-Container-c123456789abcdef".to_owned())
    );
    assert!(!engine.is_absent(OuterContainerRole::Runner));
    assert!(!engine.is_absent(OuterContainerRole::Dind));

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(proof.children().container_ids(), &[CHILD_ID.to_owned()]);
    assert!(!proof.post_actions_completed());
    assert_eq!(
        proof.cleanup_disposition(),
        CleanupDisposition::Interrupted {
            reason_class: "unknown_post_actions",
        }
    );
    Ok(())
}

#[tokio::test]
async fn ledger_refusal_for_unfenced_generation_has_no_docker_effects() -> Result<(), String> {
    let engine = FakeEngine::running();
    let ledger = FakeLedger::refusing_begin();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let result = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await;
    assert_eq!(result, Err(HostError::Identity));
    assert_eq!(engine.events(), Vec::<String>::new());
    Ok(())
}

#[tokio::test]
async fn active_runner_with_require_stopped_policy_keeps_resources() -> Result<(), String> {
    let engine = FakeEngine::running_runner();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let result = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::Interrupted {
            reason_class: "operator_deadline".to_owned(),
        },
        RunnerStopPolicy::RequireStopped,
    )
    .await;
    assert_eq!(result, Err(HostError::Docker));
    assert_eq!(engine.events(), Vec::<String>::new());
    assert!(!engine.is_absent(OuterContainerRole::Runner));
    assert!(!ledger.completed());
    Ok(())
}

#[tokio::test]
async fn forced_runner_stop_reclaims_capacity_without_marking_success() -> Result<(), String> {
    let engine = FakeEngine::running_with_forced_stop();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::Interrupted {
            reason_class: "operator_deadline".to_owned(),
        },
        RunnerStopPolicy::StopAtDeadline {
            grace_seconds: 1,
            reason_class: "operator_deadline".to_owned(),
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(proof.runner_forced());
    assert!(proof.dind_stopped());
    assert!(proof.physical_capacity_reclaimable());
    assert!(!proof.post_actions_completed());
    assert_eq!(
        proof.cleanup_disposition(),
        CleanupDisposition::Interrupted {
            reason_class: "forced_stop",
        }
    );
    Ok(())
}
