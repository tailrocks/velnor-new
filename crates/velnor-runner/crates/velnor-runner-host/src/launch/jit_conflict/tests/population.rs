// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Pair, TestResult, binding, scalar, seed};
use crate::host_controller::{
    AssignedPopulationAction, AssignedPopulationBlock, plan_assigned_population_with_successors,
};
use crate::journal::{ConnectionJournal, Journal};

pub(super) const PENDING: Pair = Pair {
    reservation: 4,
    bound: false,
};

pub(super) async fn seed_pending(journal: &Journal) -> TestResult<()> {
    let r = PENDING.reservation;
    let i = r - 1;
    let nonce = PENDING.nonce();
    let key = format!("v1|conn-test-1|7|remote|6:runner|32:{nonce}");
    let binding = scalar(
        journal,
        "SELECT binding_digest FROM physical_attempts WHERE reservation_id=2",
    )
    .await?;
    let response = format!(r#"{{"kind":"returned","request_ids":[{r}]}}"#);
    let digest = binding::digest(response.as_bytes());
    let sql = [
        format!(
            "INSERT INTO connection_request_authorizations(connection_id,scale_set_id,request_id,message_id,policy_mode,authority_model,binding_digest,source_digest,source_json,acquire_state,acquire_response_json,acquire_response_digest) SELECT connection_id,scale_set_id,{r},2,policy_mode,authority_model,binding_digest,source_digest,source_json,'acquired','{response}','{digest}' FROM connection_request_authorizations WHERE request_id=2"
        ),
        format!(
            "INSERT INTO connection_intents(id,connection_id,kind,subject,state,scale_set_id,docker_engine_id,launch_phase,assignment_key,runner_name) VALUES({i},'conn-test-1','launch','req-{i}','pending',7,'engine-test','provisioning_held','{key}','v{nonce}')"
        ),
        format!(
            "INSERT INTO capacity_reservations(id,intent_id,connection_id,assignment_key,engine_id,docker_root_digest,binding_digest,sample_generation_id,cpu_millicores,memory_bytes,state) SELECT {r},{i},connection_id,'{key}',engine_id,docker_root_digest,binding_digest,sample_generation_id,cpu_millicores,memory_bytes,'reserved' FROM capacity_reservations WHERE id=2"
        ),
        format!(
            "INSERT INTO capacity_reservation_provisioning SELECT {r},{i},profile_generation_id,profile_digest,sample_generation_id,observed_at_unix_ms,engine_id,docker_root_digest,retained_memory_bytes FROM capacity_reservation_provisioning WHERE reservation_id=2"
        ),
        format!("INSERT INTO capacity_reservation_provisioning_topology VALUES({r},2)"),
        format!(
            "INSERT INTO capacity_reservation_provisioning_state(reservation_id,bootstrap_start_state) VALUES({r},'not_requested')"
        ),
        format!(
            "INSERT INTO capacity_reservation_provisioning_pools SELECT {r},pool_id,scope,medium,filesystem_kind,inode_policy,max_bytes,max_inodes,reserve_bytes,reserve_inodes,filesystem_instance_digest,total_bytes,available_bytes,total_inodes,available_inodes FROM capacity_reservation_provisioning_pools WHERE reservation_id=2"
        ),
        format!(
            "INSERT INTO capacity_reservation_provisioning_memory SELECT {r},guest_total_bytes,guest_available_bytes FROM capacity_reservation_provisioning_memory WHERE reservation_id=2"
        ),
        format!(
            "INSERT INTO capacity_reservation_provisioning_roles SELECT {r},role,pool_id,medium,filesystem_kind,inode_policy,planned_filesystem_instance_digest,charge_bytes,charge_inodes,retained_memory_bytes FROM capacity_reservation_provisioning_roles WHERE reservation_id=2"
        ),
        format!("INSERT INTO physical_attempt_lineages VALUES({r},'conn-test-1',7,'{binding}')"),
        format!(
            "INSERT INTO physical_attempts(lineage_id,generation,reservation_id,intent_id,physical_nonce,connection_id,scale_set_id,witness_request_id,binding_digest,source_digest,acquire_response_digest,engine_id,docker_root_digest,created_at) SELECT {r},1,{r},{i},'{nonce}',connection_id,7,{r},binding_digest,source_digest,'{digest}',engine_id,docker_root_digest,0 FROM physical_attempts WHERE reservation_id=2"
        ),
        format!("INSERT INTO physical_attempt_current VALUES({r},1,{r},{i})"),
    ];
    for statement in sql {
        seed::execute(journal, &statement).await?;
    }
    for (name, role) in PENDING.volumes() {
        seed::execute(journal,&format!("INSERT INTO capacity_reservation_provisioning_objects(reservation_id,role,docker_name,publication_policy_digest,create_state) VALUES({r},'{role}','{name}',{},'unclaimed')",if role=="engine-metadata"{format!("'{}'","a".repeat(64))}else{"NULL".to_owned()})).await?;
    }
    for role in ["runner", "dind"] {
        seed::execute(journal,&format!("INSERT INTO capacity_reservation_bootstrap_containers VALUES({r},'{role}','w{nonce}-{role}',NULL,'unclaimed','not_requested')")).await?;
    }
    Ok(())
}

pub(super) struct TestPlan {
    pub(super) blocked: Option<AssignedPopulationBlock>,
    pub(super) actions: Vec<AssignedPopulationAction>,
}

pub(super) async fn plan(
    journal: &Journal,
    scope: &ConnectionJournal,
    assigned_total: i64,
) -> TestResult<TestPlan> {
    let reservations = scope.physical_runner_reservations().await?;
    let acquired = scope.acquired_authorized_requests(100).await?;
    let policy = scalar(
        journal,
        "SELECT policy_digest FROM connection_active_job_authority LIMIT 1",
    )
    .await?;
    let generation: u64 = scalar(
        journal,
        "SELECT CAST(generation AS TEXT) FROM connection_active_job_authority LIMIT 1",
    )
    .await?
    .parse()?;
    let mut candidates = Vec::new();
    let mut successors = Vec::new();
    for request in &acquired {
        if scope
            .acquired_successor_ready(request, generation, &policy)
            .await?
        {
            successors.push(request.clone());
        }
        for reservation in &reservations {
            if let Some(candidate) = scope
                .acquired_recovery_candidate(
                    request,
                    reservation.reservation_id,
                    generation,
                    &policy,
                )
                .await?
            {
                candidates.push(candidate);
            }
        }
    }
    let plan = plan_assigned_population_with_successors(
        assigned_total,
        scope.connection_id(),
        7,
        &reservations,
        &acquired,
        &candidates,
        &successors,
    );
    Ok(TestPlan {
        blocked: plan.blocked,
        actions: plan.actions,
    })
}
