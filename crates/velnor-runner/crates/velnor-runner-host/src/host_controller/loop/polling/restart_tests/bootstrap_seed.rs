// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::{ConnectionJournal, Journal, StorageRole};
use crate::launch::bootstrap::BootstrapContainerRole;
use crate::launch::bootstrap::unbound_pair_fixtures::{seed, seed::Pair};
use sha2::{Digest, Sha256};

use super::{TestResult, bootstrap_storage};

#[derive(Clone, Copy, Debug)]
pub(super) enum Crash {
    DindCreate,
    RunnerCreate,
    Start,
    VolumeCreate,
    NetworkCreate,
    MetadataRecorded,
    NetworkRecorded,
    InitializerUnfinished,
}

pub(super) async fn prepare(
    journal: &Journal,
    scope: &ConnectionJournal,
    pair: Pair,
    crash: Crash,
) -> TestResult<()> {
    capacity(journal).await?;
    let r = pair.reservation;
    let profile = bootstrap_storage::profile()?;
    let mut sql = bootstrap_storage::profile_rows(&profile, r, &"f".repeat(64))?;
    sql.extend([
        format!("UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state='not_requested',runner_container_id=NULL,dind_container_id=NULL WHERE reservation_id={r}"),
        format!("UPDATE capacity_reservation_bootstrap_containers SET create_state='unclaimed',start_state='not_requested',container_id=NULL WHERE reservation_id={r}"),
    ]);
    sql.extend(authorized_pair(pair));
    sql.extend(authorized_pair(Pair {
        reservation: 4,
        bound: false,
    }));
    if matches!(
        crash,
        Crash::NetworkCreate
            | Crash::NetworkRecorded
            | Crash::VolumeCreate
            | Crash::MetadataRecorded
            | Crash::InitializerUnfinished
    ) {
        sql.push(format!(
            "DELETE FROM capacity_reservation_networks WHERE reservation_id={r}"
        ));
    }
    if matches!(crash, Crash::VolumeCreate) {
        sql.push(format!("UPDATE capacity_reservation_provisioning_objects SET create_state='unclaimed',docker_id=NULL WHERE reservation_id={r} AND role='dind-data'"));
    }
    if matches!(crash, Crash::MetadataRecorded) {
        sql.push(format!("UPDATE capacity_reservation_provisioning_objects SET create_state='unclaimed',docker_id=NULL WHERE reservation_id={r}"));
    }
    if matches!(crash, Crash::RunnerCreate | Crash::Start) {
        sql.push(format!("UPDATE capacity_reservation_bootstrap_containers SET create_state='created',container_id='{}' WHERE reservation_id={r} AND role='dind'", pair.container_id("dind")));
    }
    if matches!(crash, Crash::Start) {
        sql.push(format!("UPDATE capacity_reservation_bootstrap_containers SET create_state='created',container_id='{}' WHERE reservation_id={r} AND role='runner'", pair.container_id("runner")));
    }
    seed::unguarded(journal, &sql).await?;
    if !matches!(crash, Crash::VolumeCreate | Crash::MetadataRecorded) {
        initialize(
            journal,
            pair,
            !matches!(crash, Crash::InitializerUnfinished),
        )
        .await?;
    }
    match crash {
        Crash::DindCreate => assert!(
            scope
                .claim_bootstrap_container_create(r, BootstrapContainerRole::Dind)
                .await?
                .is_some()
        ),
        Crash::RunnerCreate => assert!(
            scope
                .claim_bootstrap_container_create(r, BootstrapContainerRole::Runner)
                .await?
                .is_some()
        ),
        Crash::Start => assert!(scope.claim_bootstrap_start(r).await?.is_some()),
        Crash::VolumeCreate => assert!(
            scope
                .claim_storage_object_create(r, StorageRole::DindData)
                .await?
                .is_some()
        ),
        Crash::NetworkCreate => assert!(scope.claim_storage_network_create(r).await?.is_some()),
        Crash::MetadataRecorded => record_metadata(scope, r).await?,
        Crash::NetworkRecorded | Crash::InitializerUnfinished => {}
    }
    Ok(())
}

async fn initialize(journal: &Journal, pair: Pair, confirmed: bool) -> TestResult<()> {
    seed::initializer(journal, pair, "not_requested", "not_requested").await?;
    if confirmed {
        let r = pair.reservation;
        seed::unguarded(journal, &[
            format!("UPDATE capacity_reservation_storage_initialization SET start_state='started',result_state='confirmed',home_device_major=0,home_device_minor=1,home_inode=1,home_mount_id=1,work_device_major=0,work_device_minor=2,work_inode=2,work_mount_id=2,metadata_layout_digest='{}' WHERE reservation_id={r}", "a".repeat(64)),
            format!("INSERT INTO capacity_reservation_storage_handoffs (reservation_id,connection_id,operation_id,generation,helper_name,helper_id,engine_id,docker_root_digest,metadata_volume_name,metadata_volume_id,operation_key,layout_digest,ready_digest,state) SELECT reservation_id,connection_id,operation_id,generation,helper_name,helper_id,engine_id,docker_root_digest,metadata_volume,metadata_volume_id,operation_key,metadata_layout_digest,'{}','ready_held' FROM capacity_reservation_storage_initialization WHERE reservation_id={r}", "a".repeat(64)),
        ]).await?;
    }
    Ok(())
}

async fn record_metadata(scope: &ConnectionJournal, reservation: i64) -> TestResult<()> {
    let claim = scope
        .claim_storage_object_create(reservation, StorageRole::EngineMetadata)
        .await?
        .ok_or("metadata claim")?;
    let proof = crate::launch::bootstrap::VerifiedCreatedStorageObject::for_test(
        reservation,
        claim.operation_id(),
        claim.engine_id(),
        claim.docker_root_digest(),
        StorageRole::EngineMetadata,
        claim.docker_name(),
    );
    scope.record_storage_object_created(&claim, &proof).await?;
    Ok(())
}

pub(super) async fn capacity(journal: &Journal) -> TestResult<()> {
    journal
        .configure_capacity_with_operational_limits(
            crate::journal::CapacityLimits {
                jobs: 2,
                cpu_millicores: 6_000,
                memory_bytes: 12 << 30,
            },
            crate::journal::OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 1 << 30,
            },
        )
        .await?;
    Ok(())
}

fn authorized_pair(pair: Pair) -> Vec<String> {
    let source = format!(
        r#"{{"repository_id":42,"owner_name":"owner","repository_name":"repo","workflow_run_id":1,"event_name":"push","head_branch":"main","head_sha":"{}","job_workflow_ref":"owner/repo/.github/workflows/ci.yml@refs/heads/main","workflow":{{"full_ref":"owner/repo/.github/workflows/ci.yml@refs/heads/main","owner_name":"owner","repository_name":"repo","workflow_path":".github/workflows/ci.yml","reference":"refs/heads/main","kind":"branch"}},"workflow_source_sha":null,"run_attempt":1}}"#,
        "1".repeat(40)
    );
    let digest = |bytes: &[u8]| crate::digest_hex::lowercase_hex(&Sha256::digest(bytes));
    let source_digest = digest(source.as_bytes());
    let r = pair.reservation;
    let i = r - 1;
    let response = format!(r#"{{"kind":"returned","request_ids":[{r}]}}"#);
    let response_digest = digest(response.as_bytes());
    let key = format!("v1|conn-test-1|7|remote|6:runner|32:{}", pair.nonce());
    vec![
        format!(
            "INSERT INTO connection_request_authorizations (connection_id,scale_set_id,request_id,message_id,policy_mode,authority_model,binding_digest,source_digest,source_json,acquire_state,acquire_response_json,acquire_response_digest) SELECT connection_id,scale_set_id,witness_request_id,0,'branch_restricted','created_empty_exclusive_admin_session_v1',binding_digest,'{source_digest}','{source}','acquired','{response}','{response_digest}' FROM physical_attempts WHERE reservation_id={r}"
        ),
        format!(
            "UPDATE physical_attempts SET source_digest='{source_digest}',acquire_response_digest='{response_digest}' WHERE reservation_id={r}"
        ),
        format!("UPDATE connection_intents SET assignment_key='{key}' WHERE id={i}"),
        format!("UPDATE capacity_reservations SET assignment_key='{key}' WHERE id={r}"),
    ]
}

pub(super) async fn state(journal: &Journal, reservation: i64) -> TestResult<String> {
    seed::text(journal, seed::RESERVATION, reservation).await
}

pub(super) async fn charged(journal: &Journal, reservation: i64) -> TestResult<bool> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT state!='released' FROM capacity_reservations WHERE id=?1",
            [reservation],
        )
        .await?;
    Ok(rows
        .next()
        .await?
        .ok_or("reservation missing")?
        .get::<i64>(0)?
        == 1)
}

pub(super) async fn excluded_from_other_recovery(
    scope: &ConnectionJournal,
    reservation: i64,
    policy: &str,
) -> TestResult<()> {
    let request = crate::journal::AssignmentIdentity::new(
        scope.connection_id(),
        7,
        crate::journal::AssignmentKey::AssignedRequest {
            request_id: reservation,
        },
    )?;
    assert!(
        scope
            .acquired_recovery_candidate(&request, reservation, 1, policy)
            .await?
            .is_none()
    );
    assert!(scope.claim_pre_jit_cleanup(reservation).await?.is_none());
    Ok(())
}

pub(super) async fn assert_planning(
    scope: &ConnectionJournal,
    reservation: i64,
    policy: &str,
    held: bool,
) -> TestResult<()> {
    let rows = scope.physical_runner_reservations().await?;
    let neighbor = rows
        .iter()
        .find(|row| row.reservation_id == 4)
        .ok_or("bound neighbor absent from physical enumeration")?;
    assert!(matches!(
        &neighbor.ownership,
        crate::journal::PhysicalRunnerOwnership::Known(_)
    ));
    let acquired = scope.acquired_authorized_requests(2).await?;
    let request = acquired
        .iter()
        .find(|request| request.request_id() == Some(reservation))
        .ok_or("crashed request missing from acquired enumeration")?;
    let mut ready = Vec::new();
    for request in &acquired {
        if scope.acquired_successor_ready(request, 1, policy).await? {
            ready.push(request.clone());
        }
    }
    let plan = crate::host_controller::plan_assigned_population_with_successors(
        2,
        scope.connection_id(),
        7,
        &rows,
        &acquired,
        &[],
        &ready,
    );
    assert!(
        plan.actions.is_empty(),
        "crash recovery must not retry admission"
    );
    assert_eq!(
        rows.iter().any(|row| row.reservation_id == reservation),
        held
    );
    if !held {
        assert!(!ready.contains(request));
        assert_eq!(
            plan.blocked,
            Some(crate::host_controller::AssignedPopulationBlock::ClosedUnproven)
        );
    }
    Ok(())
}

pub(super) async fn bind_neighbor(
    scope: &ConnectionJournal,
    pair: Pair,
    policy: &str,
) -> TestResult<()> {
    let claim = scope
        .claim_runner_jit_for_request(
            pair.reservation - 1,
            pair.reservation,
            pair.reservation,
            &format!("v{}", pair.nonce()),
            1,
            policy,
        )
        .await?
        .ok_or("real JIT claim refused bound neighbor")?;
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
    Ok(())
}

pub(super) async fn authority(journal: &Journal) -> TestResult<String> {
    let binding = seed::text(
        journal,
        "SELECT binding_digest FROM physical_attempts WHERE reservation_id=?1",
        2,
    )
    .await?;
    let mut value = serde_json::json!({"schema_version":1,"connection_id":"conn-test-1","binding_sha256":binding,"generation":1,"policy_sha256":"0".repeat(64),"generator":{"source_sha":"1".repeat(40),"artifact_sha256":"2".repeat(64)},"workflows":[{"repository_id":42,"path":".github/workflows/ci.yml","content_sha256":"3".repeat(64),"calls":[],"jobs":[]}]});
    let policy: crate::config::ApprovedJobAuthorityPolicy = serde_json::from_value(value.clone())?;
    let digest = crate::digest_hex::lowercase_hex(&Sha256::digest(policy.canonical_json()?));
    value["policy_sha256"] = serde_json::json!(digest);
    let policy: crate::config::ApprovedJobAuthorityPolicy = serde_json::from_value(value)?;
    let json = serde_json::to_string(&policy)?;
    seed::execute(journal,&format!("INSERT INTO connection_job_authority_policies (connection_id,generation,binding_digest,policy_digest,policy_json) VALUES ('conn-test-1',1,'{binding}','{digest}','{json}')")).await?;
    seed::execute(journal,&format!("INSERT INTO connection_active_job_authority (connection_id,generation,policy_digest) VALUES ('conn-test-1',1,'{digest}')")).await?;
    seed::execute(journal,&format!("INSERT INTO connection_scale_set_intents(connection_id,binding_digest,repository_id,repository,selector_name,selector_labels,runner_group_id,state,remote_id,remote_name,remote_labels,disable_update,ownership) VALUES('conn-test-1','{binding}',42,'owner/repo','ubuntu-26.04-scale-set','[\"velnor\",\"ubuntu-26.04-scale-set\"]',1,'created',7,'ubuntu-26.04-scale-set','[\"velnor\",\"ubuntu-26.04-scale-set\"]',1,'created')")).await?;
    Ok(digest)
}
