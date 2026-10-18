use crate::journal::{ProbeLeaseState, ProbePhase, ProbeRow};
use crate::launch::resource_probe::VerifiedProbeCleanup;
use crate::launch::resource_probe::projection::{ProbeProjection, VerifiedProbeImage};

fn fixture() -> (ProbeRow, ProbeProjection, String) {
    let operation_id = "d".repeat(32);
    let container_id = "a".repeat(64);
    let projection = ProbeProjection::build(
        operation_id.clone(),
        "e".repeat(32),
        "selected-engine".to_owned(),
        "/var/lib/docker",
        VerifiedProbeImage::test_fixture(),
    )
    .expect("valid test projection");
    let row = ProbeRow {
        operation_id,
        instance_id: "e".repeat(32),
        engine_id: projection.engine_id.clone(),
        docker_root_digest: projection.root.digest().to_owned(),
        source_revision: "b".repeat(40),
        runtime_image_id: projection.image.runtime_id().to_owned(),
        image_binding_digest: "c".repeat(64),
        operation_name: projection.name.clone(),
        role: "resource-probe".to_owned(),
        projection_digest: projection.projection_digest.clone(),
        phase: ProbePhase::RemoveRequested,
        container_id: Some(container_id.clone()),
        lease_state: ProbeLeaseState::Recovering,
    };
    (row, projection, container_id)
}

#[test]
fn cleanup_proof_binds_the_persisted_operation_and_exact_object() {
    let (row, projection, id) = fixture();
    let proof = VerifiedProbeCleanup::after_verified_removal(&row, &projection, &id)
        .expect("exact removal proof");
    assert_eq!(proof.operation_id(), row.operation_id);
    assert_eq!(proof.engine_id(), row.engine_id);
    assert_eq!(proof.docker_root_digest(), row.docker_root_digest);
    assert_eq!(proof.phase(), ProbePhase::RemoveRequested);
    assert_eq!(proof.operation_name(), row.operation_name);
    assert_eq!(proof.container_id(), Some(id.as_str()));
}

#[test]
fn cleanup_proof_rejects_changed_operation_scope_or_container() {
    let (row, projection, id) = fixture();
    let mut changed_operation = row.clone();
    changed_operation.operation_id = "f".repeat(32);
    assert!(
        VerifiedProbeCleanup::after_verified_removal(&changed_operation, &projection, &id).is_err()
    );

    let mut changed_engine = row.clone();
    changed_engine.engine_id.push_str("-other");
    assert!(
        VerifiedProbeCleanup::after_verified_removal(&changed_engine, &projection, &id).is_err()
    );

    let mut changed_root = row.clone();
    changed_root.docker_root_digest = "f".repeat(64);
    if changed_root.docker_root_digest == row.docker_root_digest {
        changed_root.docker_root_digest = "e".repeat(64);
    }
    assert!(VerifiedProbeCleanup::after_verified_removal(&changed_root, &projection, &id).is_err());

    let mut changed_name = row.clone();
    changed_name.operation_name.push_str("-other");
    assert!(VerifiedProbeCleanup::after_verified_removal(&changed_name, &projection, &id).is_err());

    let mut wrong_phase = row.clone();
    wrong_phase.phase = ProbePhase::CreateRequested;
    assert!(VerifiedProbeCleanup::after_verified_removal(&wrong_phase, &projection, &id).is_err());

    let mut unbound = row;
    unbound.container_id = None;
    assert!(VerifiedProbeCleanup::after_verified_removal(&unbound, &projection, &id).is_err());
}
