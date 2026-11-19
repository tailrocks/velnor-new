// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::controller::Controller;
use super::{CRASHED, Crash, NEIGHBOR, Prepared, TestResult, bootstrap_seed, db, prepare};

#[path = "inbox_fake.rs"]
mod fake;

#[tokio::test(flavor = "multi_thread")]
async fn draining_completion_unlocks_bootstrap_retirement_without_another_restart() -> TestResult<()>
{
    if fake::isolated(concat!(
        module_path!(),
        "::draining_completion_unlocks_bootstrap_retirement_without_another_restart"
    ))? {
        return Ok(());
    }
    for crash in [Crash::DindCreate, Crash::NetworkCreate] {
        Box::pin(recovers_after_drain(crash))
            .await
            .map_err(|error| format!("{crash:?}: {error}"))?;
    }
    Ok(())
}

async fn recovers_after_drain(crash: Crash) -> TestResult<()> {
    let case = prepare(crash, false).await?;
    let response = r#"{"kind":"returned","request_ids":[50]}"#;
    let digest = fake::digest(response.as_bytes());
    let connection = case.journal.connection().await?;
    connection.execute(&format!("INSERT INTO connection_request_authorizations(connection_id,scale_set_id,request_id,message_id,policy_mode,authority_model,binding_digest,source_digest,source_json,acquire_state,acquire_response_json,acquire_response_digest) SELECT connection_id,scale_set_id,50,50,policy_mode,authority_model,binding_digest,source_digest,source_json,'acquired','{response}','{digest}' FROM connection_request_authorizations WHERE request_id=2"), ()).await?;
    drop(connection);
    let body = serde_json::json!([{"messageType":"JobCompleted","runnerRequestId":50,"jobId":"job-50","ownerName":"owner","repositoryName":"repo","jobWorkflowRef":"owner/repo/.github/workflows/ci.yml@refs/heads/main","workflowRunId":1,"eventName":"push","result":"success"}]).to_string();
    case.scope.store_completion_inbox(80, &body).await?;
    let owned = case.fixture.objects(CRASHED);
    let neighbor = bootstrap_seed::state(&case.journal, NEIGHBOR.reservation).await?;
    let neighbor_objects = case.fixture.objects(NEIGHBOR);
    let case = case.restart().await?;
    assert_classified_and_fenced(&case, crash).await?;
    assert!(case.fixture.has_initializer(CRASHED));
    let initializer = initializer_state(&case.journal).await?;
    let proofs = super::super::seed::cleanup_proven(&case.journal, CRASHED.reservation).await?;
    assert_eq!(proofs, (0, 0, 0));
    assert_eq!(case.journal.readiness_capacity_usage().await?.jobs, 2);
    let mut controller = Controller::new(&case.scope, &case.engine)?;
    case.fixture.take_requests();
    controller
        .turn(&case.journal, &case.docker, &case.engine)
        .await?;
    assert!(bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    assert_eq!(case.journal.readiness_capacity_usage().await?.jobs, 2);
    assert_eq!(
        super::super::seed::cleanup_proven(&case.journal, CRASHED.reservation).await?,
        proofs
    );
    assert!(case.fixture.has_initializer(CRASHED));
    assert_eq!(initializer_state(&case.journal).await?, initializer);
    assert_eq!(case.fixture.objects(CRASHED), owned);
    assert_eq!(case.fixture.objects(NEIGHBOR), neighbor_objects);
    let requests = case.fixture.take_requests();
    assert!(
        requests
            .iter()
            .all(|request| !["POST ", "DELETE ", "PATCH "]
                .iter()
                .any(|method| request.starts_with(method))),
        "queued completion allowed Docker effects: {requests:?}"
    );
    fake::poll(&case, &body).await?;
    assert_eq!(
        db::number(
            &case.journal,
            "SELECT count(*) FROM connection_completion_inbox"
        )
        .await?,
        0
    );
    assert_eq!(db::number(&case.journal, "SELECT count(*) FROM connection_request_authorizations WHERE request_id=50 AND completed_message_id=80 AND completed_event_digest IS NOT NULL AND completed_job_id='job-50' AND completed_result='success' AND completed_runner_id IS NULL AND completed_runner_name IS NULL AND completed_intent_id IS NULL").await?, 1);
    controller
        .turn(&case.journal, &case.docker, &case.engine)
        .await?;
    db::released(&case, &neighbor, &neighbor_objects).await
}

async fn assert_classified_and_fenced(case: &Prepared, crash: Crash) -> TestResult<()> {
    assert_eq!(db::number(&case.journal, "SELECT count(*) FROM capacity_reservations r JOIN connection_intents i ON i.id=r.intent_id WHERE r.id=2 AND r.state='uncertain' AND i.state='uncertain'").await?, 1);
    let classified = match crash {
        Crash::DindCreate => {
            "SELECT count(*) FROM capacity_reservation_bootstrap_containers WHERE reservation_id=2 AND role='dind' AND create_state='uncertain' AND start_state='not_requested' AND container_id IS NULL"
        }
        Crash::NetworkCreate => {
            "SELECT count(*) FROM capacity_reservation_networks WHERE reservation_id=2 AND create_state='uncertain' AND network_id IS NULL"
        }
        _ => return Err("unsupported inbox crash".into()),
    };
    assert_eq!(db::number(&case.journal, classified).await?, 1);
    assert_eq!(db::number(&case.journal, "SELECT count(*) FROM capacity_reservation_provisioning_state WHERE reservation_id=2 AND bootstrap_start_state='not_requested'").await?, 1);
    assert!(
        case.scope
            .claim_unbound_retirement(CRASHED.reservation)
            .await?
            .is_none()
    );
    assert!(
        case.scope
            .storage_initializer_abort_candidate(CRASHED.reservation)
            .await?
            .is_none()
    );
    Ok(())
}

async fn initializer_state(journal: &crate::journal::Journal) -> TestResult<String> {
    let connection = journal.connection().await?;
    let mut rows = connection.query("SELECT json_array(create_dispatch_state,create_state,start_state,result_state,remove_state,abort_state,abort_claim_generation,abort_lease_until,helper_id,abort_helper_id) FROM capacity_reservation_storage_initialization WHERE reservation_id=2", ()).await?;
    let state = rows.next().await?.ok_or("initializer state")?.get(0)?;
    assert!(rows.next().await?.is_none());
    Ok(state)
}
