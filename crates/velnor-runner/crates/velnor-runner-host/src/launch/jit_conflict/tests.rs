// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;
use crate::launch::bootstrap::unbound_pair_fixtures::{fixture::FixtureDocker, seed, seed::Pair};

mod binding;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const CRASH: Pair = Pair {
    reservation: 2,
    bound: false,
};

#[tokio::test(flavor = "multi_thread")]
async fn restart_quarantines_a_committed_jit_claim_without_fabricating_a_conflict() -> TestResult<()>
{
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[CRASH])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let path = dir.path().join("launch.db");
    let journal = Journal::open(&path).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[CRASH]).await?;
    seed::bind_resources(&journal, CRASH, engine.docker_root_digest()).await?;
    binding::install(&journal, CRASH).await?;
    seed_claim(&journal, CRASH).await?;
    assert_eq!(
        shape(&journal).await?,
        "reserved/pending/jit_requested/claimed/requested"
    );
    let owned = fixture.objects(CRASH);
    journal
        .recover_after_restart_without_authority()
        .await
        .map_err(|error| format!("startup recovery failed: {error}"))?;

    assert_eq!(
        shape(&journal).await?,
        "uncertain/uncertain/jit_uncertain/uncertain/uncertain"
    );
    assert_eq!(fixture.objects(CRASH), owned);
    assert_eq!(
        scalar(
            &journal,
            "SELECT count(*) FROM physical_attempt_jit_conflict_receipts"
        )
        .await?,
        "0"
    );
    journal.recover_after_restart_without_authority().await?;
    drop(scope);
    drop(journal);
    let journal = Journal::open(&path)
        .await
        .map_err(|error| format!("reopening the crashed journal failed: {error}"))?;
    assert_eq!(
        shape(&journal).await?,
        "uncertain/uncertain/jit_uncertain/uncertain/uncertain"
    );
    Ok(())
}

async fn seed_claim(journal: &Journal, pair: Pair) -> TestResult<()> {
    let binding = scalar(
        journal,
        "SELECT binding_digest FROM physical_attempts LIMIT 1",
    )
    .await?;
    let policy_value = serde_json::json!({"schema_version":1,"connection_id":"conn-test-1","binding_sha256":binding,"generation":1,"policy_sha256":"0".repeat(64),"generator":{"source_sha":"1".repeat(40),"artifact_sha256":"2".repeat(64)},"workflows":[{"repository_id":42,"path":".github/workflows/ci.yml","content_sha256":"3".repeat(64),"calls":[],"jobs":[]}]});
    let policy: crate::config::ApprovedJobAuthorityPolicy =
        serde_json::from_value(policy_value.clone())?;
    let authority = binding::digest(&policy.canonical_json()?);
    let mut policy_value = policy_value;
    policy_value["policy_sha256"] = serde_json::json!(authority);
    let policy: crate::config::ApprovedJobAuthorityPolicy = serde_json::from_value(policy_value)?;
    let policy_json = serde_json::to_string(&policy)?;
    let source = format!(
        r#"{{"repository_id":42,"owner_name":"owner","repository_name":"repo","workflow_run_id":1,"event_name":"push","head_branch":"main","head_sha":"{}","job_workflow_ref":"owner/repo/.github/workflows/ci.yml@refs/heads/main","workflow":{{"full_ref":"owner/repo/.github/workflows/ci.yml@refs/heads/main","owner_name":"owner","repository_name":"repo","workflow_path":".github/workflows/ci.yml","reference":"refs/heads/main","kind":"branch"}},"workflow_source_sha":null,"run_attempt":1}}"#,
        "1".repeat(40)
    );
    let source_digest = binding::digest(source.as_bytes());
    let response = format!(
        r#"{{"kind":"returned","request_ids":[{}]}}"#,
        pair.reservation
    );
    let response_digest = binding::digest(response.as_bytes());
    let key = format!("v1|conn-test-1|7|remote|6:runner|32:{}", pair.nonce());
    for sql in [
        format!(
            "INSERT INTO connection_job_authority_policies (connection_id,generation,binding_digest,policy_digest,policy_json) VALUES ('conn-test-1',1,'{binding}','{authority}','{policy_json}')"
        ),
        format!(
            "INSERT INTO connection_active_job_authority (connection_id,generation,policy_digest) VALUES ('conn-test-1',1,'{authority}')"
        ),
        format!(
            "INSERT INTO connection_request_authorizations (connection_id,scale_set_id,request_id,message_id,policy_mode,authority_model,binding_digest,source_digest,source_json,acquire_state,acquire_response_json,acquire_response_digest) VALUES ('conn-test-1',7,{},1,'branch_restricted','created_empty_exclusive_admin_session_v1','{binding}','{source_digest}','{source}','acquired','{response}','{response_digest}')",
            pair.reservation
        ),
        format!(
            "INSERT INTO connection_runner_jits (intent_id,connection_id,scale_set_id,witness_request_id,requested_name,state) VALUES ({},'conn-test-1',7,{},'v{}','claimed')",
            pair.reservation - 1,
            pair.reservation,
            pair.nonce()
        ),
        format!(
            "UPDATE connection_intents SET assignment_key='{key}',jit_requested=1,launch_phase='jit_requested',docker_id='{}',dind_id='{}' WHERE id={}",
            pair.container_id("runner"),
            pair.container_id("dind"),
            pair.reservation - 1
        ),
        format!(
            "UPDATE capacity_reservation_provisioning_state SET jit_state='requested' WHERE reservation_id={}",
            pair.reservation
        ),
    ] {
        seed::execute(journal, &sql).await?;
    }
    seed::execute(
        journal,
        &format!(
            "UPDATE capacity_reservations SET assignment_key='{key}' WHERE id={}",
            pair.reservation
        ),
    )
    .await?;
    seed::unguarded(
        journal,
        &[format!(
            "UPDATE physical_attempts SET source_digest='{source_digest}' WHERE reservation_id={}",
            pair.reservation
        )],
    )
    .await?;
    Ok(())
}

async fn shape(journal: &Journal) -> TestResult<String> {
    scalar(journal,"SELECT r.state||'/'||i.state||'/'||i.launch_phase||'/'||j.state||'/'||s.jit_state FROM capacity_reservations r JOIN connection_intents i ON i.id=r.intent_id JOIN connection_runner_jits j ON j.intent_id=i.id JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id WHERE r.id=2").await
}

async fn scalar(journal: &Journal, sql: &str) -> TestResult<String> {
    let connection = journal.connection().await?;
    let mut rows = connection.query(sql, ()).await?;
    Ok(rows.next().await?.ok_or("row missing")?.get(0)?)
}
