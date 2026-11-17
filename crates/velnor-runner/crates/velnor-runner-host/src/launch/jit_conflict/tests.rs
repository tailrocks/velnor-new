// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;
use crate::launch::bootstrap::unbound_pair_fixtures::{fixture::FixtureDocker, seed, seed::Pair};

mod binding;
mod case;
mod end_to_end;
mod github;
mod historical;
mod population;
mod probe_docker;
mod storage_seed;

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
    let bound = Pair {
        reservation: 4,
        bound: false,
    };
    let fixture = FixtureDocker::start(&socket, &[CRASH, bound])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let path = dir.path().join("launch.db");
    let journal = Journal::open(&path).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[CRASH, bound]).await?;
    seed::bind_resources(&journal, CRASH, engine.docker_root_digest()).await?;
    binding::install(&journal, CRASH).await?;
    storage_seed::install(&journal, CRASH).await?;
    seed_claim(&journal, CRASH).await?;
    seed::bind_resources(&journal, bound, engine.docker_root_digest()).await?;
    binding::install(&journal, bound).await?;
    storage_seed::install(&journal, bound).await?;
    seed_jit(&journal, bound, JitOutcome::Bound).await?;
    assert!(matches!(
        scope
            .physical_runner_reservations()
            .await?
            .first()
            .ok_or("physical reservation")?
            .ownership,
        crate::journal::PhysicalRunnerOwnership::Known(_)
    ));
    assert_eq!(
        shape(&journal).await?,
        "reserved/pending/jit_requested/claimed/requested"
    );
    let owned = fixture.objects(CRASH);
    let bound_before=scalar(&journal,"SELECT i.state||'/'||i.launch_phase||'/'||i.github_runner_id||'/'||j.state||'/'||s.jit_state FROM connection_intents i JOIN connection_runner_jits j ON j.intent_id=i.id JOIN capacity_reservation_provisioning_state s ON s.reservation_id=4 WHERE i.id=3").await?;
    assert_eq!(bound_before, "pending/jit_bound/21/bound/bound");
    journal
        .recover_after_restart_without_authority()
        .await
        .map_err(|error| format!("startup recovery failed: {error}"))?;

    assert_eq!(
        shape(&journal).await?,
        "uncertain/uncertain/jit_uncertain/uncertain/uncertain"
    );
    assert_eq!(fixture.objects(CRASH), owned);
    assert_eq!(fixture.objects(bound).len(), 8);
    assert_eq!(scalar(&journal,"SELECT i.state||'/'||i.launch_phase||'/'||i.github_runner_id||'/'||j.state||'/'||s.jit_state FROM connection_intents i JOIN connection_runner_jits j ON j.intent_id=i.id JOIN capacity_reservation_provisioning_state s ON s.reservation_id=4 WHERE i.id=3").await?,bound_before);
    assert_eq!(
        scalar(
            &journal,
            "SELECT CAST(count(*) AS TEXT) FROM physical_attempt_jit_conflict_receipts"
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

enum JitOutcome {
    Interrupted,
    Conflict,
    Bound,
}

async fn seed_claim(journal: &Journal, pair: Pair) -> TestResult<()> {
    seed_jit(journal, pair, JitOutcome::Interrupted).await
}

async fn seed_jit(journal: &Journal, pair: Pair, outcome: JitOutcome) -> TestResult<()> {
    let authority = seed_authorized_bound_request(journal, pair).await?;
    let config: crate::config::RepositoryConnection = serde_json::from_str(
        &scalar(journal, "SELECT identity FROM connection_scopes LIMIT 1").await?,
    )?;
    let scope = journal.connection_scope(&config).await?;
    let claim = scope
        .claim_runner_jit_for_request(
            pair.reservation - 1,
            pair.reservation,
            pair.reservation,
            &format!("v{}", pair.nonce()),
            1,
            &authority,
        )
        .await?
        .ok_or("real JIT claim refused complete bound fixture")?;
    match outcome {
        JitOutcome::Interrupted => {}
        JitOutcome::Conflict => {
            scope
                .mark_runner_jit_conflict_received(claim, pair.reservation, 1, &authority)
                .await?;
        }
        JitOutcome::Bound => {
            scope
                .bind_jit_runner(
                    claim,
                    &velnor_runner_github::RunnerReference {
                        id: 21,
                        name: format!("v{}", pair.nonce()),
                        runner_scale_set_id: 7,
                    },
                )
                .await?;
        }
    }
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

fn job_policy(binding: &str) -> TestResult<crate::config::ApprovedJobAuthorityPolicy> {
    let policy_value = serde_json::json!({"schema_version":1,"connection_id":"conn-test-1","binding_sha256":binding,"generation":1,"policy_sha256":"0".repeat(64),"generator":{"source_sha":"1".repeat(40),"artifact_sha256":"2".repeat(64)},"workflows":[{"repository_id":42,"path":".github/workflows/ci.yml","content_sha256":"3".repeat(64),"calls":[],"jobs":[]}]});
    let policy: crate::config::ApprovedJobAuthorityPolicy =
        serde_json::from_value(policy_value.clone())?;
    let authority = binding::digest(&policy.canonical_json()?);
    let mut policy_value = policy_value;
    policy_value["policy_sha256"] = serde_json::json!(authority);
    let policy: crate::config::ApprovedJobAuthorityPolicy = serde_json::from_value(policy_value)?;
    Ok(policy)
}

fn source_json() -> String {
    format!(
        r#"{{"repository_id":42,"owner_name":"owner","repository_name":"repo","workflow_run_id":1,"event_name":"push","head_branch":"main","head_sha":"{}","job_workflow_ref":"owner/repo/.github/workflows/ci.yml@refs/heads/main","workflow":{{"full_ref":"owner/repo/.github/workflows/ci.yml@refs/heads/main","owner_name":"owner","repository_name":"repo","workflow_path":".github/workflows/ci.yml","reference":"refs/heads/main","kind":"branch"}},"workflow_source_sha":null,"run_attempt":1}}"#,
        "1".repeat(40)
    )
}

async fn seed_authorized_bound_request(journal: &Journal, pair: Pair) -> TestResult<String> {
    let binding = scalar(
        journal,
        "SELECT binding_digest FROM physical_attempts LIMIT 1",
    )
    .await?;
    let policy = job_policy(&binding)?;
    let authority = policy.policy_sha256().to_owned();
    let policy_json = serde_json::to_string(&policy)?;
    let source = source_json();
    let source_digest = binding::digest(source.as_bytes());
    let response = format!(
        r#"{{"kind":"returned","request_ids":[{}]}}"#,
        pair.reservation
    );
    let response_digest = binding::digest(response.as_bytes());
    let key = format!("v1|conn-test-1|7|remote|6:runner|32:{}", pair.nonce());
    for sql in [
        format!(
            "INSERT OR IGNORE INTO connection_job_authority_policies (connection_id,generation,binding_digest,policy_digest,policy_json) VALUES ('conn-test-1',1,'{binding}','{authority}','{policy_json}')"
        ),
        format!(
            "INSERT OR IGNORE INTO connection_active_job_authority (connection_id,generation,policy_digest) VALUES ('conn-test-1',1,'{authority}')"
        ),
        format!(
            "INSERT INTO connection_request_authorizations (connection_id,scale_set_id,request_id,message_id,policy_mode,authority_model,binding_digest,source_digest,source_json,acquire_state,acquire_response_json,acquire_response_digest) VALUES ('conn-test-1',7,{},1,'branch_restricted','created_empty_exclusive_admin_session_v1','{binding}','{source_digest}','{source}','acquired','{response}','{response_digest}')",
            pair.reservation
        ),
        format!(
            "UPDATE connection_intents SET assignment_key='{key}',launch_phase='resources_bound',docker_id='{}',dind_id='{}' WHERE id={}",
            pair.container_id("runner"),
            pair.container_id("dind"),
            pair.reservation - 1
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
    seed::execute(journal,&format!("INSERT OR IGNORE INTO connection_scale_set_intents(connection_id,binding_digest,repository_id,repository,selector_name,selector_labels,runner_group_id,state,remote_id,remote_name,remote_labels,disable_update,ownership) VALUES('conn-test-1','{binding}',42,'owner/repo','ubuntu-26.04-scale-set','[\"velnor\",\"ubuntu-26.04-scale-set\"]',1,'created',7,'ubuntu-26.04-scale-set','[\"velnor\",\"ubuntu-26.04-scale-set\"]',1,'created')")).await?;
    Ok(authority)
}
