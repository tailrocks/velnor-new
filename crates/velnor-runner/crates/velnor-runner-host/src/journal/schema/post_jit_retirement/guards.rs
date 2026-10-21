//! Database triggers that check retirement evidence and prevent changes to its records.

pub(super) const CLAIM_INSERT_GUARD: &str = r"CREATE TRIGGER physical_attempt_post_jit_retirement_claim_insert_guard BEFORE INSERT ON physical_attempt_post_jit_retirement_claims
WHEN NOT EXISTS (
    SELECT 1
    FROM physical_attempt_current cur
    JOIN physical_attempts a ON a.lineage_id=cur.lineage_id AND a.generation=cur.generation AND a.reservation_id=cur.reservation_id AND a.intent_id=cur.intent_id
    JOIN capacity_reservations r ON r.id=a.reservation_id AND r.intent_id=a.intent_id AND r.connection_id=a.connection_id
    JOIN connection_intents i ON i.id=a.intent_id AND i.connection_id=a.connection_id
    JOIN capacity_reservation_provisioning p ON p.reservation_id=r.id AND p.operation_id=i.id
    JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id
    JOIN connection_runner_jits j ON j.intent_id=i.id AND j.connection_id=i.connection_id
    JOIN connection_request_authorizations auth ON auth.connection_id=a.connection_id AND auth.scale_set_id=a.scale_set_id AND auth.request_id=a.witness_request_id
    JOIN connection_runtime_bindings binding ON binding.connection_id=a.connection_id
    LEFT JOIN physical_attempt_post_jit_retirement_claims prior ON prior.reservation_id=NEW.reservation_id AND prior.claim_generation=NEW.claim_generation-1
    WHERE a.lineage_id=NEW.lineage_id AND a.generation=NEW.generation AND a.reservation_id=NEW.reservation_id AND a.intent_id=NEW.intent_id
      AND a.connection_id=NEW.connection_id AND a.scale_set_id=NEW.scale_set_id AND a.witness_request_id=NEW.witness_request_id
      AND a.binding_digest=NEW.binding_digest AND a.source_digest=NEW.source_digest
      AND a.engine_id=NEW.engine_id AND a.docker_root_digest=NEW.docker_root_digest
      AND binding.scale_set_id=a.scale_set_id AND auth.binding_digest=a.binding_digest AND auth.source_digest=a.source_digest
      AND auth.acquire_response_digest=NEW.acquire_response_digest AND auth.acquire_state='acquired'
      AND auth.acquire_response_json IS NOT NULL
      AND (a.acquire_response_digest IS NULL OR a.acquire_response_digest=auth.acquire_response_digest)
      AND r.state IN ('reserved','uncertain','recovering') AND r.binding_digest=a.binding_digest AND r.engine_id=a.engine_id AND r.docker_root_digest=a.docker_root_digest
      AND i.kind='launch' AND i.scale_set_id=a.scale_set_id AND i.runner_name=NEW.runner_name AND i.github_runner_id=CAST(NEW.runner_id AS TEXT)
      AND p.engine_id=a.engine_id AND p.docker_root_digest=a.docker_root_digest
      AND s.instance_binding_digest=NEW.instance_binding_digest AND s.runner_id=NEW.runner_id AND s.runner_name=NEW.runner_name AND s.runner_scale_set_id=NEW.scale_set_id
      AND j.runner_id=NEW.runner_id AND j.runner_name=NEW.runner_name AND j.scale_set_id=NEW.scale_set_id AND j.witness_request_id=NEW.witness_request_id
      AND NEW.lease_until>unixepoch() AND NEW.created_at=unixepoch()
      AND NEW.claim_generation=COALESCE((SELECT MAX(latest.claim_generation)+1 FROM physical_attempt_post_jit_retirement_claims latest WHERE latest.reservation_id=NEW.reservation_id),1)
      AND NOT EXISTS (SELECT 1 FROM physical_attempt_outcomes prior_outcome WHERE prior_outcome.lineage_id=a.lineage_id AND prior_outcome.generation=a.generation AND prior_outcome.reservation_id=a.reservation_id)
      AND NOT EXISTS (SELECT 1 FROM physical_attempt_post_jit_retirements retired WHERE retired.lineage_id=a.lineage_id AND retired.generation=a.generation AND retired.reservation_id=a.reservation_id)
      AND (
        (NEW.claim_generation=1
         AND i.cleanup_proven=0 AND i.runner_completed=0 AND i.runner_request_id IS NULL
         AND j.state='ready' AND j.started_request_id IS NULL AND j.completed_request_id IS NULL AND j.requested_name=NEW.runner_name
         AND s.bootstrap_start_state='started' AND s.resources_bound=1 AND s.storage_cleanup_proven=0 AND s.jit_state='bound'
         AND s.publication_state='published' AND s.delivery_state='delivered'
         AND i.assignment_key=r.assignment_key AND NEW.runner_name='v'||a.physical_nonce
         AND EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers bc WHERE bc.reservation_id=r.id AND bc.role='runner' AND bc.create_state='created' AND bc.start_state='started' AND bc.container_id=s.runner_container_id)
         AND EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers bc WHERE bc.reservation_id=r.id AND bc.role='dind' AND bc.create_state='created' AND bc.start_state='started' AND bc.container_id=s.dind_container_id)
         AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_objects obj WHERE obj.reservation_id=r.id AND obj.create_state!='created')
         AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects obj WHERE obj.reservation_id=r.id AND obj.create_state='created')=5
         AND EXISTS (SELECT 1 FROM capacity_reservation_networks net WHERE net.reservation_id=r.id AND net.connection_id=a.connection_id AND net.operation_id=i.id AND net.engine_id=a.engine_id AND net.docker_root_digest=a.docker_root_digest AND net.create_state='created' AND net.network_id IS NOT NULL AND net.remove_state='not_requested')
         AND NOT EXISTS (SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=i.id)
         AND NOT EXISTS (SELECT 1 FROM connection_request_authorizations actual WHERE actual.connection_id=a.connection_id AND actual.scale_set_id=a.scale_set_id AND ((actual.started_event_digest IS NOT NULL AND actual.started_runner_id=j.runner_id AND actual.started_runner_name=j.runner_name) OR (actual.completed_event_digest IS NOT NULL AND actual.completed_runner_id=j.runner_id AND actual.completed_runner_name=j.runner_name)))
         AND EXISTS (SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=r.id AND proof.connection_id=a.connection_id AND proof.scale_set_id=a.scale_set_id AND proof.witness_request_id=a.witness_request_id AND proof.binding_digest=a.binding_digest AND proof.engine_id=a.engine_id AND proof.docker_root_digest=a.docker_root_digest AND proof.instance_binding_digest=s.instance_binding_digest AND proof.runner_id=j.runner_id AND proof.runner_name=j.runner_name AND proof.proof_state='pending' AND proof.actual_request_id IS NULL AND proof.started_event_digest IS NULL AND proof.challenge_state='published')
         AND EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization init JOIN capacity_reservation_storage_handoffs handoff ON handoff.reservation_id=init.reservation_id AND handoff.connection_id=init.connection_id AND handoff.operation_id=init.operation_id AND handoff.generation=init.generation AND handoff.helper_name=init.helper_name AND handoff.helper_id=init.helper_id AND handoff.engine_id=init.engine_id AND handoff.docker_root_digest=init.docker_root_digest AND handoff.metadata_volume_name=init.metadata_volume AND handoff.metadata_volume_id=init.metadata_volume_id AND handoff.operation_key=init.operation_key AND handoff.layout_digest=init.metadata_layout_digest WHERE init.reservation_id=r.id AND init.connection_id=a.connection_id AND init.operation_id=i.id AND init.engine_id=a.engine_id AND init.docker_root_digest=a.docker_root_digest AND init.create_dispatch_state='response_received' AND init.create_state='created' AND init.start_state='started' AND init.result_state='confirmed' AND init.remove_state='removed' AND init.abort_state='not_requested' AND init.abort_lease_until=0 AND init.layout_version=2 AND handoff.state='attached' AND handoff.runner_container_id=s.runner_container_id AND handoff.dind_container_id=s.dind_container_id)
         AND NOT EXISTS (SELECT 1 FROM execution_publication_operations publication JOIN resource_probe_operations op ON op.operation_id=publication.operation_id WHERE publication.reservation_id=r.id AND op.phase NOT IN ('removed','aborted')))
        OR
        (NEW.claim_generation>1 AND prior.lease_until<=unixepoch() AND prior.lineage_id=NEW.lineage_id AND prior.generation=NEW.generation AND prior.intent_id=NEW.intent_id AND prior.connection_id=NEW.connection_id AND prior.scale_set_id=NEW.scale_set_id AND prior.witness_request_id=NEW.witness_request_id AND prior.binding_digest=NEW.binding_digest AND prior.source_digest=NEW.source_digest AND prior.acquire_response_digest=NEW.acquire_response_digest AND prior.engine_id=NEW.engine_id AND prior.docker_root_digest=NEW.docker_root_digest AND prior.instance_binding_digest=NEW.instance_binding_digest AND prior.runner_id=NEW.runner_id AND prior.runner_name=NEW.runner_name AND prior.snapshot_sha256=NEW.snapshot_sha256)
      )
)
BEGIN SELECT RAISE(ABORT,'The physical retirement claim is invalid.'); END";

pub(super) fn claim_insert_guard_v37() -> Result<String, crate::error::HostError> {
    let recovering = CLAIM_INSERT_GUARD.replace(
        "(NEW.claim_generation=1\n         AND i.cleanup_proven=0",
        "(NEW.claim_generation=1\n         AND r.state='recovering'\n         AND i.cleanup_proven=0",
    );
    if recovering == CLAIM_INSERT_GUARD {
        return Err(crate::error::HostError::Journal);
    }
    let marker = recovering.replace(
        "proof.proof_state='pending' AND proof.actual_request_id IS NULL AND proof.started_event_digest IS NULL AND proof.challenge_state='published'",
        "proof.proof_state='denied' AND proof.deny_code='interrupted_after_restart' AND proof.actual_request_id IS NULL AND proof.started_event_digest IS NULL AND proof.challenge_state='abandoned'",
    );
    if marker == recovering {
        return Err(crate::error::HostError::Journal);
    }
    Ok(marker)
}

pub(super) const PROGRESS_INSERT_GUARD: &str = r"CREATE TRIGGER physical_attempt_post_jit_retirement_progress_insert_guard BEFORE INSERT ON physical_attempt_post_jit_retirement_progress
WHEN NOT EXISTS (SELECT 1 FROM physical_attempt_post_jit_retirement_claims c
 WHERE c.reservation_id=NEW.reservation_id AND c.claim_generation=NEW.claim_generation
 AND c.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_post_jit_retirement_claims latest WHERE latest.reservation_id=c.reservation_id)
 AND c.lease_until>unixepoch() AND NEW.created_at=unixepoch()
 AND NEW.phase_index=COALESCE((SELECT MAX(p.phase_index)+1 FROM physical_attempt_post_jit_retirement_progress p WHERE p.reservation_id=c.reservation_id AND p.claim_generation=c.claim_generation),0)
 AND (NEW.phase_index!=0 OR NEW.evidence_sha256=c.snapshot_sha256)
 AND (NEW.phase_index!=2 OR EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_remote_absence remote WHERE remote.reservation_id=c.reservation_id AND remote.claim_generation=c.claim_generation AND remote.runner_id=c.runner_id AND remote.runner_name=c.runner_name AND remote.scale_set_id=c.scale_set_id))
 AND (NEW.phase_index!=3 OR EXISTS(SELECT 1 FROM capacity_reservation_provisioning_cleanups cleanup JOIN capacity_reservation_provisioning_state s ON s.reservation_id=cleanup.reservation_id WHERE cleanup.reservation_id=c.reservation_id AND cleanup.instance_binding_digest=c.instance_binding_digest AND s.instance_binding_digest=c.instance_binding_digest AND s.storage_cleanup_proven=1 AND cleanup.runner_container_id=s.runner_container_id AND cleanup.dind_container_id=s.dind_container_id AND cleanup.role_mask=2047 AND cleanup.containers_absent=1 AND cleanup.volumes_absent=1 AND cleanup.mounts_detached=1 AND cleanup.cgroups_absent=1))
 AND (NEW.phase_index!=4 OR EXISTS(SELECT 1 FROM capacity_reservation_networks n WHERE n.reservation_id=c.reservation_id AND n.connection_id=c.connection_id AND n.operation_id=c.intent_id AND n.engine_id=c.engine_id AND n.docker_root_digest=c.docker_root_digest AND n.remove_state='removed'))
 AND (NEW.phase_index!=5 OR EXISTS(SELECT 1 FROM capacity_reservation_metadata_settlement_claims mc JOIN capacity_reservation_metadata_settlements ms ON ms.reservation_id=mc.reservation_id AND ms.generation=mc.generation AND ms.probe_operation_id=mc.probe_operation_id WHERE mc.reservation_id=c.reservation_id AND mc.operation_id=c.intent_id AND mc.connection_id=c.connection_id AND mc.scale_set_id=c.scale_set_id AND mc.engine_id=c.engine_id AND mc.docker_root_digest=c.docker_root_digest AND mc.instance_binding_digest=c.instance_binding_digest AND mc.generation=(SELECT MAX(latest.generation) FROM capacity_reservation_metadata_settlement_claims latest WHERE latest.reservation_id=c.reservation_id) AND ms.outcome='settled')))
BEGIN SELECT RAISE(ABORT,'The physical retirement phase is invalid.'); END";

pub(super) const REMOTE_ABSENCE_INSERT_GUARD: &str = r"CREATE TRIGGER physical_attempt_post_jit_retirement_remote_absence_insert_guard BEFORE INSERT ON physical_attempt_post_jit_retirement_remote_absence
WHEN NOT EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims c WHERE c.reservation_id=NEW.reservation_id AND c.claim_generation=NEW.claim_generation
 AND c.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_post_jit_retirement_claims latest WHERE latest.reservation_id=c.reservation_id)
 AND c.lease_until>unixepoch() AND NEW.runner_id=c.runner_id AND NEW.runner_name=c.runner_name AND NEW.scale_set_id=c.scale_set_id
 AND NEW.observed_at_unix_ms>=c.created_at*1000 AND NEW.observed_at_unix_ms<=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER)
 AND EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_progress p WHERE p.reservation_id=c.reservation_id AND p.claim_generation=c.claim_generation AND p.phase_index=1 AND p.phase='runner_stopped'))
BEGIN SELECT RAISE(ABORT,'The runner absence receipt is invalid.'); END";

pub(super) const RETIREMENT_INSERT_GUARD: &str = r"CREATE TRIGGER physical_attempt_post_jit_retirement_insert_guard BEFORE INSERT ON physical_attempt_post_jit_retirements
BEGIN
    SELECT RAISE(ABORT,'The physical retirement proof is invalid.') WHERE NOT EXISTS (
        SELECT 1 FROM physical_attempt_post_jit_retirement_claims c
        JOIN physical_attempt_current cur ON cur.lineage_id=c.lineage_id AND cur.generation=c.generation AND cur.reservation_id=c.reservation_id AND cur.intent_id=c.intent_id
        JOIN physical_attempts a ON a.lineage_id=c.lineage_id AND a.generation=c.generation AND a.reservation_id=c.reservation_id AND a.intent_id=c.intent_id
        JOIN capacity_reservations r ON r.id=a.reservation_id AND r.intent_id=a.intent_id AND r.connection_id=a.connection_id
        JOIN connection_intents i ON i.id=a.intent_id AND i.connection_id=a.connection_id
        JOIN capacity_reservation_provisioning p ON p.reservation_id=r.id AND p.operation_id=i.id
        JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id
        JOIN connection_runner_jits j ON j.intent_id=i.id AND j.connection_id=i.connection_id
        WHERE c.lineage_id=NEW.lineage_id AND c.generation=NEW.generation AND c.reservation_id=NEW.reservation_id AND c.claim_generation=NEW.claim_generation
          AND c.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_post_jit_retirement_claims latest WHERE latest.reservation_id=c.reservation_id)
          AND c.lease_until>unixepoch() AND cur.lineage_id=NEW.lineage_id AND cur.generation=NEW.generation AND cur.reservation_id=NEW.reservation_id AND cur.intent_id=c.intent_id
          AND a.intent_id=c.intent_id AND a.connection_id=c.connection_id AND a.scale_set_id=c.scale_set_id AND a.binding_digest=c.binding_digest
          AND a.witness_request_id=c.witness_request_id AND a.source_digest=c.source_digest
          AND (a.acquire_response_digest IS NULL OR a.acquire_response_digest=c.acquire_response_digest) AND a.engine_id=c.engine_id AND a.docker_root_digest=c.docker_root_digest
          AND r.state IN ('reserved','uncertain','recovering') AND r.remote_cleanup_proven=1 AND r.local_cleanup_proven=1 AND r.binding_digest=c.binding_digest AND r.engine_id=c.engine_id AND r.docker_root_digest=c.docker_root_digest
          AND i.kind='launch' AND i.scale_set_id=c.scale_set_id AND i.cleanup_proven=1 AND i.runner_name=c.runner_name AND i.github_runner_id=CAST(c.runner_id AS TEXT)
          AND p.engine_id=c.engine_id AND p.docker_root_digest=c.docker_root_digest AND s.bootstrap_start_state='started' AND s.resources_bound=1 AND s.storage_cleanup_proven=1 AND s.jit_state='cleaned'
          AND s.instance_binding_digest=c.instance_binding_digest AND s.runner_id=c.runner_id AND s.runner_name=c.runner_name AND s.runner_scale_set_id=c.scale_set_id
          AND j.state='cleaned' AND j.scale_set_id=c.scale_set_id AND j.witness_request_id=c.witness_request_id AND j.runner_id=c.runner_id AND j.runner_name=c.runner_name
          AND NOT EXISTS(SELECT 1 FROM physical_attempt_outcomes prior_outcome WHERE prior_outcome.lineage_id=c.lineage_id AND prior_outcome.generation=c.generation AND prior_outcome.reservation_id=c.reservation_id)
          AND EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_progress phase WHERE phase.reservation_id=c.reservation_id AND phase.claim_generation=c.claim_generation AND phase.phase_index=5 AND phase.phase='capacity_settled')
    );
    SELECT RAISE(ABORT,'The physical retirement proof is invalid.') WHERE NOT EXISTS (
        SELECT 1 FROM physical_attempt_post_jit_retirement_claims c
        JOIN physical_attempt_post_jit_retirement_remote_absence remote ON remote.reservation_id=c.reservation_id AND remote.claim_generation=c.claim_generation
        JOIN capacity_reservation_provisioning_cleanups cleanup ON cleanup.reservation_id=c.reservation_id
        JOIN capacity_reservation_provisioning_state s ON s.reservation_id=c.reservation_id
        JOIN capacity_reservation_cleanup_observer_witnesses w ON w.reservation_id=c.reservation_id AND w.connection_id=c.connection_id AND w.operation_id=c.intent_id
        JOIN capacity_reservation_cleanup_observer_probes cp ON cp.reservation_id=w.reservation_id AND cp.connection_id=w.connection_id AND cp.operation_id=w.operation_id AND cp.binding_sha256=w.binding_sha256 AND cp.probe_operation_id=NEW.cleanup_probe_operation_id
        JOIN resource_probe_operations observer ON observer.operation_id=cp.probe_operation_id
        WHERE c.lineage_id=NEW.lineage_id AND c.generation=NEW.generation AND c.reservation_id=NEW.reservation_id AND c.claim_generation=NEW.claim_generation
          AND remote.runner_id=c.runner_id AND remote.runner_name=c.runner_name AND remote.scale_set_id=c.scale_set_id
          AND remote.observed_at_unix_ms>=c.created_at*1000
          AND cleanup.instance_binding_digest=c.instance_binding_digest AND cleanup.runner_container_id=s.runner_container_id AND cleanup.dind_container_id=s.dind_container_id
          AND cleanup.role_mask=2047 AND cleanup.containers_absent=1 AND cleanup.volumes_absent=1 AND cleanup.mounts_detached=1 AND cleanup.cgroups_absent=1
          AND w.engine_id=c.engine_id AND w.docker_root_digest=c.docker_root_digest AND w.instance_binding_digest=c.instance_binding_digest
          AND w.runner_container_id=s.runner_container_id AND w.dind_container_id=s.dind_container_id AND w.guest_boot_id=s.guest_boot_id
          AND observer.probe_purpose='cleanup-observer' AND observer.phase='removed' AND observer.engine_id=c.engine_id AND observer.docker_root_digest=c.docker_root_digest
    );
    SELECT RAISE(ABORT,'The physical retirement proof is invalid.') WHERE NOT EXISTS (
        SELECT 1 FROM physical_attempt_post_jit_retirement_claims c
        JOIN capacity_reservation_networks n ON n.reservation_id=c.reservation_id
        WHERE c.lineage_id=NEW.lineage_id AND c.generation=NEW.generation AND c.reservation_id=NEW.reservation_id AND c.claim_generation=NEW.claim_generation
          AND n.connection_id=c.connection_id AND n.operation_id=c.intent_id AND n.engine_id=c.engine_id AND n.docker_root_digest=c.docker_root_digest
          AND n.create_state='created' AND n.network_id IS NOT NULL AND n.remove_state='removed'
          AND NOT EXISTS(SELECT 1 FROM execution_publication_operations publication JOIN resource_probe_operations op ON op.operation_id=publication.operation_id WHERE publication.reservation_id=c.reservation_id AND op.phase NOT IN ('removed','aborted'))
    );
    SELECT RAISE(ABORT,'The physical retirement proof is invalid.') WHERE NOT EXISTS (
        SELECT 1 FROM physical_attempt_post_jit_retirement_claims c
        JOIN physical_attempt_post_jit_retirement_remote_absence remote ON remote.reservation_id=c.reservation_id AND remote.claim_generation=c.claim_generation
        JOIN capacity_reservation_metadata_settlement_claims mc ON mc.reservation_id=c.reservation_id AND mc.generation=NEW.metadata_generation
        JOIN capacity_reservation_metadata_settlements ms ON ms.reservation_id=mc.reservation_id AND ms.generation=mc.generation AND ms.probe_operation_id=mc.probe_operation_id
        JOIN capacity_reservation_metadata_probe_cleanups mpc ON mpc.reservation_id=mc.reservation_id AND mpc.generation=mc.generation AND mpc.probe_operation_id=mc.probe_operation_id
        JOIN resource_probe_operations mp ON mp.operation_id=mc.probe_operation_id
        JOIN host_operation_leases lease ON lease.singleton=1 AND lease.operation_id=mp.operation_id
        JOIN host_storage_profile_current pc ON pc.singleton=1
        JOIN host_storage_profiles profile ON profile.generation_id=pc.generation_id
        JOIN journal_meta meta ON meta.singleton=1
        WHERE c.lineage_id=NEW.lineage_id AND c.generation=NEW.generation AND c.reservation_id=NEW.reservation_id AND c.claim_generation=NEW.claim_generation
          AND mc.operation_id=c.intent_id AND mc.connection_id=c.connection_id AND mc.scale_set_id=c.scale_set_id AND mc.instance_id=meta.instance_id
          AND mc.engine_id=c.engine_id AND mc.docker_root_digest=c.docker_root_digest AND mc.instance_binding_digest=c.instance_binding_digest
          AND mc.generation=(SELECT MAX(latest.generation) FROM capacity_reservation_metadata_settlement_claims latest WHERE latest.reservation_id=c.reservation_id)
          AND mc.profile_generation_id=profile.generation_id AND mc.profile_digest=profile.profile_digest
          AND ms.outcome='settled' AND ms.sample_generation_id=mc.probe_operation_id AND ms.profile_generation_id=mc.profile_generation_id AND ms.profile_digest=mc.profile_digest
          AND ms.engine_id=mc.engine_id AND ms.docker_root_digest=mc.docker_root_digest
          AND mpc.phase='removed' AND mpc.lease_released=1 AND mpc.engine_id=mc.engine_id AND mpc.docker_root_digest=mc.docker_root_digest
          AND mp.phase='removed' AND mp.probe_purpose='capacity' AND mp.engine_id=mc.engine_id AND mp.docker_root_digest=mc.docker_root_digest
          AND lease.state='released' AND lease.engine_id=mc.engine_id AND lease.docker_root_digest=mc.docker_root_digest
          AND ms.observed_at_unix_ms>=remote.observed_at_unix_ms
    );
    SELECT RAISE(ABORT,'The physical retirement proof is invalid.') WHERE NOT EXISTS (
        SELECT 1 FROM physical_attempt_post_jit_retirement_claims c
        JOIN physical_attempt_post_jit_retirement_remote_absence remote ON remote.reservation_id=c.reservation_id AND remote.claim_generation=c.claim_generation
        JOIN capacity_reservation_metadata_settlements ms ON ms.reservation_id=c.reservation_id AND ms.generation=NEW.metadata_generation
        WHERE c.lineage_id=NEW.lineage_id AND c.generation=NEW.generation AND c.reservation_id=NEW.reservation_id AND c.claim_generation=NEW.claim_generation
          AND remote.observed_at_unix_ms>=c.created_at*1000
          AND ms.observed_at_unix_ms>=remote.observed_at_unix_ms
          AND ms.observed_at_unix_ms<=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER)
          AND ms.observed_at_unix_ms>=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER)-30000
    );
    SELECT RAISE(ABORT,'The physical retirement proof is invalid.') WHERE NOT EXISTS (
        SELECT 1 FROM physical_attempt_post_jit_retirement_claims c
        WHERE c.lineage_id=NEW.lineage_id AND c.generation=NEW.generation AND c.reservation_id=NEW.reservation_id AND c.claim_generation=NEW.claim_generation
          AND EXISTS(SELECT 1 FROM capacity_reservation_provisioning_pools rp WHERE rp.reservation_id=c.reservation_id AND rp.scope='shared-disk' AND rp.medium='disk')
          AND NOT EXISTS(SELECT 1 FROM capacity_reservation_provisioning_pools rp WHERE rp.reservation_id=c.reservation_id AND rp.scope='shared-disk' AND rp.medium='disk' AND NOT EXISTS(SELECT 1 FROM capacity_reservation_metadata_settlement_pools sp WHERE sp.reservation_id=c.reservation_id AND sp.generation=NEW.metadata_generation AND sp.pool_id=rp.pool_id AND sp.filesystem_instance_digest=rp.filesystem_instance_digest AND sp.medium='disk'))
          AND NOT EXISTS(SELECT 1 FROM capacity_reservation_metadata_settlement_pools sp WHERE sp.reservation_id=c.reservation_id AND sp.generation=NEW.metadata_generation AND NOT EXISTS(SELECT 1 FROM capacity_reservation_provisioning_pools rp WHERE rp.reservation_id=c.reservation_id AND rp.scope='shared-disk' AND rp.medium='disk' AND rp.pool_id=sp.pool_id AND rp.filesystem_instance_digest=sp.filesystem_instance_digest))
    );
END";

pub(super) const OUTCOME_RETIREMENT_FENCE: &str = r"CREATE TRIGGER physical_attempt_outcome_retirement_fence BEFORE INSERT ON physical_attempt_outcomes
WHEN EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims c WHERE c.lineage_id=NEW.lineage_id AND c.generation=NEW.generation AND c.reservation_id=NEW.reservation_id)
 OR EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirements retired WHERE retired.lineage_id=NEW.lineage_id AND retired.generation=NEW.generation AND retired.reservation_id=NEW.reservation_id)
BEGIN SELECT RAISE(ABORT,'The physical attempt has a retirement fence.'); END";

pub(super) const CLAIMS_NO_UPDATE: &str = "CREATE TRIGGER physical_attempt_post_jit_retirement_claims_no_update BEFORE UPDATE ON physical_attempt_post_jit_retirement_claims BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END";
pub(super) const CLAIMS_NO_DELETE: &str = "CREATE TRIGGER physical_attempt_post_jit_retirement_claims_no_delete BEFORE DELETE ON physical_attempt_post_jit_retirement_claims BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END";
pub(super) const PROGRESS_NO_UPDATE: &str = "CREATE TRIGGER physical_attempt_post_jit_retirement_progress_no_update BEFORE UPDATE ON physical_attempt_post_jit_retirement_progress BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END";
pub(super) const PROGRESS_NO_DELETE: &str = "CREATE TRIGGER physical_attempt_post_jit_retirement_progress_no_delete BEFORE DELETE ON physical_attempt_post_jit_retirement_progress BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END";
pub(super) const REMOTE_ABSENCE_NO_UPDATE: &str = "CREATE TRIGGER physical_attempt_post_jit_retirement_remote_absence_no_update BEFORE UPDATE ON physical_attempt_post_jit_retirement_remote_absence BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END";
pub(super) const REMOTE_ABSENCE_NO_DELETE: &str = "CREATE TRIGGER physical_attempt_post_jit_retirement_remote_absence_no_delete BEFORE DELETE ON physical_attempt_post_jit_retirement_remote_absence BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END";
pub(super) const RETIREMENTS_NO_UPDATE: &str = "CREATE TRIGGER physical_attempt_post_jit_retirements_no_update BEFORE UPDATE ON physical_attempt_post_jit_retirements BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END";
pub(super) const RETIREMENTS_NO_DELETE: &str = "CREATE TRIGGER physical_attempt_post_jit_retirements_no_delete BEFORE DELETE ON physical_attempt_post_jit_retirements BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END";
