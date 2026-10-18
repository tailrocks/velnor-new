//! Acquire and JIT admission state regressions.

use super::*;
use crate::connect::SetOwnership;

#[tokio::test]
async fn acquire_needs_receipt_and_uncertain_result_survives_restart() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 4_000,
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let binding = binding("essential", 101, "donbeave/essential-mac")?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (assignment, intent_id) = reserve_assigned(&scope, 44).await?;
    assert!(!scope.claim_assigned_acquire(intent_id).await?);
    let receipt = test_receipt(44, 88, source(101, 901), SourcePolicyMode::Repository);
    scope
        .record_source_authorization(&assignment, &receipt)
        .await?;
    assert!(scope.claim_assigned_acquire(intent_id).await?);
    assert!(!scope.claim_assigned_acquire(intent_id).await?);
    scope
        .record_assigned_acquire(intent_id, AssignedAcquireResult::Uncertain)
        .await?;
    assert!(!scope.claim_launch_jit(intent_id).await?);
    assert!(!journal.is_drained().await?);
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let (intent_id, fresh) = scope
        .begin_assigned_launch(
            &scope
                .assignment_identity(AssignmentKey::AssignedRequest { request_id: 44 })
                .await?,
            "v44",
        )
        .await?;
    assert!(!fresh);
    assert!(!scope.claim_assigned_acquire(intent_id).await?);
    assert!(!scope.claim_launch_jit(intent_id).await?);
    assert!(!journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn only_receipted_closed_acquire_can_claim_jit() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 4_000,
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let binding = binding("essential", 101, "donbeave/essential-mac")?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (assignment, intent_id) = reserve_assigned(&scope, 55).await?;
    let receipt = test_receipt(55, 99, source(101, 902), SourcePolicyMode::Repository);
    scope
        .record_source_authorization(&assignment, &receipt)
        .await?;
    assert!(scope.claim_assigned_acquire(intent_id).await?);
    scope
        .record_assigned_acquire(intent_id, AssignedAcquireResult::Returned(vec![55]))
        .await?;
    assert!(scope.claim_launch_jit(intent_id).await?);
    assert!(!scope.claim_launch_jit(intent_id).await?);
    Ok(())
}

#[tokio::test]
async fn unattempted_assignment_without_receipt_blocks_closed_world_jit() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: 8_000,
            memory_bytes: 17_179_869_184,
        })
        .await?;
    let binding = binding("essential", 101, "donbeave/essential-mac")?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (_, unreceipted_id) = reserve_assigned(&scope, 72).await?;
    let (assignment, ready_id) = reserve_assigned(&scope, 73).await?;
    let receipt = test_receipt(73, 106, source(101, 909), SourcePolicyMode::Repository);
    scope
        .record_source_authorization(&assignment, &receipt)
        .await?;
    assert!(scope.claim_assigned_acquire(ready_id).await?);
    scope
        .record_assigned_acquire(ready_id, AssignedAcquireResult::Returned(vec![73]))
        .await?;

    assert!(!scope.claim_launch_jit(ready_id).await?);
    assert!(!scope.claim_assigned_acquire(unreceipted_id).await?);
    Ok(())
}

#[tokio::test]
async fn empty_acquire_reply_is_uncertain_not_a_no_effect() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 4_000,
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let binding = binding("essential", 101, "donbeave/essential-mac")?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (assignment, intent_id) = reserve_assigned(&scope, 67).await?;
    let receipt = test_receipt(67, 101, source(101, 904), SourcePolicyMode::Repository);
    scope
        .record_source_authorization(&assignment, &receipt)
        .await?;
    assert!(scope.claim_assigned_acquire(intent_id).await?);
    scope
        .record_assigned_acquire(intent_id, AssignedAcquireResult::Returned(Vec::new()))
        .await?;
    assert!(!scope.claim_assigned_acquire(intent_id).await?);
    assert!(!scope.claim_launch_jit(intent_id).await?);
    assert!(!journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn unresolved_acquire_for_another_request_blocks_jit() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: 8_000,
            memory_bytes: 17_179_869_184,
        })
        .await?;
    let binding = binding("essential", 101, "donbeave/essential-mac")?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (uncertain_assignment, uncertain_id) = reserve_assigned(&scope, 70).await?;
    let uncertain_receipt = test_receipt(70, 104, source(101, 907), SourcePolicyMode::Repository);
    scope
        .record_source_authorization(&uncertain_assignment, &uncertain_receipt)
        .await?;
    assert!(scope.claim_assigned_acquire(uncertain_id).await?);
    scope
        .record_assigned_acquire(uncertain_id, AssignedAcquireResult::Uncertain)
        .await?;

    let (ready_assignment, ready_id) = reserve_assigned(&scope, 71).await?;
    let ready_receipt = test_receipt(71, 105, source(101, 908), SourcePolicyMode::Repository);
    scope
        .record_source_authorization(&ready_assignment, &ready_receipt)
        .await?;
    assert!(scope.claim_assigned_acquire(ready_id).await?);
    scope
        .record_assigned_acquire(ready_id, AssignedAcquireResult::Returned(vec![71]))
        .await?;
    assert!(!scope.claim_launch_jit(ready_id).await?);
    Ok(())
}

#[tokio::test]
async fn active_unscoped_legacy_intent_blocks_scoped_jit() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 4_000,
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let binding = binding("essential", 101, "donbeave/essential-mac")?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (assignment, intent_id) = reserve_assigned(&scope, 68).await?;
    let receipt = test_receipt(68, 102, source(101, 905), SourcePolicyMode::Repository);
    scope
        .record_source_authorization(&assignment, &receipt)
        .await?;
    assert!(scope.claim_assigned_acquire(intent_id).await?);
    scope
        .record_assigned_acquire(intent_id, AssignedAcquireResult::Returned(vec![68]))
        .await?;
    let _legacy_id = journal.begin("launch", "legacy").await?;
    assert!(!scope.claim_launch_jit(intent_id).await?);
    Ok(())
}

#[tokio::test]
async fn one_remote_set_id_cannot_be_scoped_to_two_connections() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 4_000,
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let first_binding = binding("essential", 101, "donbeave/essential-mac")?;
    let first = created_scope(&journal, &first_binding, 501).await?;
    let (assignment, intent_id) = reserve_assigned(&first, 69).await?;
    let receipt = test_receipt(69, 103, source(101, 906), SourcePolicyMode::Repository);
    first
        .record_source_authorization(&assignment, &receipt)
        .await?;
    assert!(first.claim_assigned_acquire(intent_id).await?);
    first
        .record_assigned_acquire(intent_id, AssignedAcquireResult::Returned(vec![69]))
        .await?;

    let second_binding = binding("other", 202, "other/project")?;
    let second = journal.connection_scope(&second_binding).await?;
    second
        .bind_scale_set(501, "integration", SetOwnership::Adopted)
        .await?;
    second
        .bind_engine_context("test-engine", "unix:///var/run/docker.sock", "linux/amd64")
        .await?;
    assert!(!first.claim_launch_jit(intent_id).await?);
    Ok(())
}

#[tokio::test]
async fn unassigned_live_row_blocks_closed_world_jit() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: 8_000,
            memory_bytes: 17_179_869_184,
        })
        .await?;
    let binding = binding("essential", 101, "donbeave/essential-mac")?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (assignment, intent_id) = reserve_assigned(&scope, 66).await?;
    let receipt = test_receipt(66, 100, source(101, 903), SourcePolicyMode::Repository);
    scope
        .record_source_authorization(&assignment, &receipt)
        .await?;
    assert!(scope.claim_assigned_acquire(intent_id).await?);
    scope
        .record_assigned_acquire(intent_id, AssignedAcquireResult::Returned(vec![66]))
        .await?;

    let population = scope
        .assignment_identity(AssignmentKey::PopulationMessage { message_id: 101 })
        .await?;
    let permit = StartPermit::test_fixture();
    let generation = permit.sample_generation_id().to_owned();
    let root_digest = permit.docker_root_digest().to_owned();
    assert!(matches!(
        scope
            .try_reserve(
                ReservationRequest {
                    assignment: &population,
                    engine_id: "test-engine",
                    docker_root_digest: &root_digest,
                    cpu_millicores: 2_000,
                    memory_bytes: 8_589_934_592,
                    sample_generation_id: &generation,
                },
                permit,
            )
            .await?,
        ReservationResult::Reserved { .. }
    ));
    assert!(!scope.claim_launch_jit(intent_id).await?);
    Ok(())
}
