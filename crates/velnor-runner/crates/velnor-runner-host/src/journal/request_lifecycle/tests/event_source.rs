//! Native event-source validation and restart-safe correlation regressions.

use super::helpers::{
    Scratch, binding, created_scope, event_source, receipt, record_acquired, runner_name,
};
use crate::error::HostError;
use crate::journal::{
    AssignmentIdentity, AssignmentKey, CapacityLimits, ConnectionJournal, Journal, ReservationState,
};
use velnor_runner_github::{RunnerReference, SourceMetadata};

struct StartedFixture {
    binding: crate::config::RepositoryConnection,
    scope: ConnectionJournal,
    physical: AssignmentIdentity,
    intent_id: i64,
    request: AssignmentIdentity,
    source: velnor_runner_github::SourceMetadata,
    runner: RunnerReference,
}

#[tokio::test]
async fn started_event_replay_works_before_and_after_completion_and_restart()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let fixture = started_fixture(&journal).await?;
    reject_changed_sources(&fixture).await?;
    record_and_clean_started_runner(&fixture).await?;
    let binding = fixture.binding.clone();
    let physical = fixture.physical.clone();
    let intent_id = fixture.intent_id;
    let request = fixture.request.clone();
    let source = fixture.source.clone();
    let runner = fixture.runner.clone();
    drop(fixture);
    drop(journal);

    assert_started_replay_after_restart(
        &scratch.file(),
        &binding,
        &physical,
        intent_id,
        &request,
        &source,
        &runner,
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn absent_event_source_fields_match_authorization_but_presence_is_replay_identity()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let fixture = started_fixture(&journal).await?;
    let absent = SourceMetadata::default();
    assert_absent_start_replay_and_presence(&fixture, &absent).await?;
    assert_absent_completion_replay_and_presence(&fixture, &absent).await?;

    let binding = fixture.binding.clone();
    let physical = fixture.physical.clone();
    let intent_id = fixture.intent_id;
    let request = fixture.request.clone();
    let runner = fixture.runner.clone();
    drop(fixture);
    drop(journal);

    assert_replay_survives_restart(
        &path, &binding, &physical, intent_id, &request, &runner, &absent,
    )
    .await?;
    Ok(())
}

async fn assert_absent_start_replay_and_presence(
    fixture: &StartedFixture,
    absent: &SourceMetadata,
) -> Result<(), HostError> {
    for _ in 0..2 {
        assert_eq!(
            fixture
                .scope
                .record_job_started(&fixture.request, 910, absent, &fixture.runner, None)
                .await?,
            fixture.intent_id
        );
    }
    let present = SourceMetadata {
        owner_name: fixture.source.owner_name.clone(),
        ..SourceMetadata::default()
    };
    assert!(matches!(
        fixture
            .scope
            .record_job_started(&fixture.request, 910, &present, &fixture.runner, None)
            .await,
        Err(HostError::Identity)
    ));
    Ok(())
}

async fn assert_absent_completion_replay_and_presence(
    fixture: &StartedFixture,
    absent: &SourceMetadata,
) -> Result<(), HostError> {
    for _ in 0..2 {
        assert_eq!(
            fixture
                .scope
                .record_job_completed(
                    &fixture.request,
                    911,
                    absent,
                    Some("cancelled"),
                    Some(&fixture.runner),
                    None,
                )
                .await?,
            Some(fixture.intent_id)
        );
    }
    let present = SourceMetadata {
        owner_name: fixture.source.owner_name.clone(),
        ..SourceMetadata::default()
    };
    assert!(matches!(
        fixture
            .scope
            .record_job_completed(
                &fixture.request,
                911,
                &present,
                Some("cancelled"),
                Some(&fixture.runner),
                None,
            )
            .await,
        Err(HostError::Identity)
    ));
    Ok(())
}

async fn assert_replay_survives_restart(
    path: &std::path::Path,
    binding: &crate::config::RepositoryConnection,
    physical: &AssignmentIdentity,
    intent_id: i64,
    request: &AssignmentIdentity,
    runner: &RunnerReference,
    absent: &SourceMetadata,
) -> Result<(), HostError> {
    let journal = Journal::open(path).await?;
    let scope = journal.connection_scope(binding).await?;
    assert_eq!(
        scope
            .record_job_started(request, 910, absent, runner, None)
            .await?,
        intent_id
    );
    assert_eq!(
        scope
            .record_job_completed(request, 911, absent, Some("cancelled"), Some(runner), None,)
            .await?,
        Some(intent_id)
    );
    assert_eq!(
        scope.reservation(physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

async fn started_fixture(journal: &Journal) -> Result<StartedFixture, HostError> {
    let binding = binding("essential", 101)?;
    let scope = created_scope(journal, &binding, 501).await?;
    record_acquired(&scope, 91).await?;
    let (physical, intent_id) = super::admission::physical_intent(&scope, 'c').await?;
    let claim = scope
        .claim_runner_jit(intent_id, &runner_name('c'))
        .await?
        .ok_or(HostError::Journal)?;
    let runner = RunnerReference {
        id: 791,
        name: runner_name('c'),
        runner_scale_set_id: 501,
    };
    scope.bind_jit_runner(claim, &runner).await?;
    let start = scope
        .claim_physical_runner_start(intent_id, &format!("w{}", "c".repeat(32)))
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .record_physical_worker_ready(start, "runner-c", "dind-c")
        .await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 91 })
        .await?;
    Ok(StartedFixture {
        binding,
        scope,
        physical,
        intent_id,
        request,
        source: event_source(101, 1_091),
        runner,
    })
}

async fn reject_changed_sources(fixture: &StartedFixture) -> Result<(), HostError> {
    for source in changed_sources(&fixture.source)
        .into_iter()
        .chain(invalid_present_sources(&fixture.source))
    {
        assert!(matches!(
            fixture
                .scope
                .record_job_started(
                    &fixture.request,
                    901,
                    &source,
                    &fixture.runner,
                    Some("opaque-job")
                )
                .await,
            Err(HostError::Identity)
        ));
        assert!(matches!(
            fixture
                .scope
                .record_job_completed(
                    &fixture.request,
                    902,
                    &source,
                    Some("success"),
                    Some(&fixture.runner),
                    Some("opaque-job")
                )
                .await,
            Err(HostError::Identity)
        ));
    }
    Ok(())
}

fn changed_sources(
    source: &velnor_runner_github::SourceMetadata,
) -> Vec<velnor_runner_github::SourceMetadata> {
    let mut mismatches = Vec::new();
    let mut changed = source.clone();
    changed.owner_name = Some("other-owner".to_owned());
    mismatches.push(changed);
    let mut changed = source.clone();
    changed.repository_name = Some("other-repository".to_owned());
    mismatches.push(changed);
    let mut changed = source.clone();
    changed.job_workflow_ref = Some("other/workflow.yml@refs/heads/main".to_owned());
    mismatches.push(changed);
    let mut changed = source.clone();
    changed.workflow_run_id = Some(1_092);
    mismatches.push(changed);
    let mut changed = source.clone();
    changed.event_name = Some("workflow_dispatch".to_owned());
    mismatches.push(changed);
    mismatches
}

fn invalid_present_sources(
    source: &velnor_runner_github::SourceMetadata,
) -> Vec<velnor_runner_github::SourceMetadata> {
    let mut invalid = Vec::new();
    let mut changed = source.clone();
    changed.owner_name = Some(String::new());
    invalid.push(changed);
    let mut changed = source.clone();
    changed.repository_name = Some(String::new());
    invalid.push(changed);
    let mut changed = source.clone();
    changed.job_workflow_ref = Some(String::new());
    invalid.push(changed);
    let mut changed = source.clone();
    changed.workflow_run_id = Some(0);
    invalid.push(changed);
    let mut changed = source.clone();
    changed.event_name = Some(String::new());
    invalid.push(changed);
    invalid
}

async fn record_and_clean_started_runner(fixture: &StartedFixture) -> Result<(), HostError> {
    let scope = &fixture.scope;
    assert_started_once(fixture).await?;
    assert_started_once(fixture).await?;
    assert_eq!(
        scope
            .record_job_completed(
                &fixture.request,
                902,
                &fixture.source,
                Some("success"),
                Some(&fixture.runner),
                Some("opaque-job")
            )
            .await?,
        Some(fixture.intent_id)
    );
    clean_and_release_runner(fixture).await?;
    assert_started_once(fixture).await?;
    Ok(())
}

async fn assert_started_once(fixture: &StartedFixture) -> Result<(), HostError> {
    assert_eq!(
        fixture
            .scope
            .record_job_started(
                &fixture.request,
                901,
                &fixture.source,
                &fixture.runner,
                Some("opaque-job"),
            )
            .await?,
        fixture.intent_id
    );
    Ok(())
}

async fn clean_and_release_runner(fixture: &StartedFixture) -> Result<(), HostError> {
    let scope = &fixture.scope;
    let claim = scope
        .claim_completion_cleanup_at(fixture.intent_id, 0, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        scope
            .run_completion_cleanup_effect_at(fixture.intent_id, claim.generation, 1, 5, || async {
                Ok::<_, HostError>(())
            })
            .await?,
        Some(())
    );
    assert!(
        scope
            .record_completion_runner_absent_at(fixture.intent_id, claim.generation, 2)
            .await?
    );
    assert!(
        scope
            .record_completion_cleanup_at(fixture.intent_id, claim.generation, 3)
            .await?
    );
    let reservation_id = scope
        .reservation(&fixture.physical)
        .await?
        .ok_or(HostError::Journal)?
        .reservation_id;
    scope.record_remote_cleanup(reservation_id).await?;
    scope.record_local_cleanup(reservation_id).await?;
    Ok(())
}

async fn assert_started_replay_after_restart(
    path: &std::path::Path,
    binding: &crate::config::RepositoryConnection,
    physical: &AssignmentIdentity,
    intent_id: i64,
    request: &AssignmentIdentity,
    source: &velnor_runner_github::SourceMetadata,
    runner: &RunnerReference,
) -> Result<(), HostError> {
    let journal = Journal::open(path).await?;
    let scope = journal.connection_scope(binding).await?;
    assert_eq!(
        scope
            .record_job_started(request, 901, source, runner, Some("opaque-job"))
            .await?,
        intent_id
    );
    assert_eq!(
        scope.reservation(physical).await?.map(|row| row.state),
        Some(ReservationState::Released)
    );
    Ok(())
}

#[tokio::test]
async fn authorized_runnerless_completion_survives_restart_and_blocks_acquire()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 92 })
        .await?;
    let source_receipt = receipt(&scope, 92, 1_092, 101).await?;
    scope
        .record_source_authorization(&request, &source_receipt)
        .await?;
    let event_source = event_source(101, 1_092);
    assert_eq!(
        scope
            .record_job_completed(
                &request,
                903,
                &event_source,
                Some("cancelled"),
                None,
                Some("opaque-job"),
            )
            .await?,
        None
    );
    assert!(!scope.claim_request_acquire(&request).await?);
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 92 })
        .await?;
    assert_eq!(
        scope
            .record_job_completed(
                &request,
                903,
                &event_source,
                Some("cancelled"),
                None,
                Some("opaque-job"),
            )
            .await?,
        None
    );
    assert!(!scope.claim_request_acquire(&request).await?);
    Ok(())
}
