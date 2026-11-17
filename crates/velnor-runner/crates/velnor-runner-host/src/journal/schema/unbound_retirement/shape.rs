// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(in crate::journal) const UNBOUND_SHAPE: &str = "EXISTS (SELECT 1 FROM capacity_reservation_unbound_candidates candidate WHERE candidate.reservation_id={s}.reservation_id)";

const CONTAINERS: &str = "CREATE VIEW capacity_reservation_unbound_container_shapes AS SELECT s.reservation_id, CASE WHEN (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers c WHERE c.reservation_id=s.reservation_id)=2 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers c WHERE c.reservation_id=s.reservation_id AND (c.create_state!='created' OR c.container_id IS NULL)) THEN 1 ELSE 0 END AS pair_created, CASE WHEN (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers c WHERE c.reservation_id=s.reservation_id)=2 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers c WHERE c.reservation_id=s.reservation_id AND (c.start_state!='not_requested' OR c.create_state NOT IN ('unclaimed','created','uncertain'))) AND EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers c WHERE c.reservation_id=s.reservation_id AND c.create_state IN ('created','uncertain')) THEN 1 ELSE 0 END AS pair_attempted, CASE WHEN (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers c WHERE c.reservation_id=s.reservation_id) IN (0,2) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers c WHERE c.reservation_id=s.reservation_id AND (c.create_state!='unclaimed' OR c.start_state!='not_requested' OR c.container_id IS NOT NULL)) THEN 1 ELSE 0 END AS no_effects FROM capacity_reservation_provisioning_state s";

const INITIALIZERS: &str = "CREATE VIEW capacity_reservation_unbound_initializer_ready AS SELECT s.reservation_id FROM capacity_reservation_provisioning_state s WHERE NOT EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization init WHERE init.reservation_id=s.reservation_id) UNION SELECT init.reservation_id FROM capacity_reservation_storage_initialization init WHERE init.abort_state='absent' UNION SELECT init.reservation_id FROM capacity_reservation_storage_initialization init JOIN capacity_reservation_storage_handoffs handoff ON handoff.reservation_id=init.reservation_id AND handoff.state='ready_held' JOIN capacity_reservation_provisioning_objects object ON object.reservation_id=init.reservation_id AND object.role='engine-metadata' AND object.create_state='created' AND object.docker_name=init.metadata_volume AND object.docker_id=init.metadata_volume_id WHERE init.abort_state='not_requested' AND init.abort_lease_until=0 AND init.layout_version=2 AND init.create_state='created' AND init.start_state='started' AND init.result_state='confirmed' AND init.remove_state='not_requested' AND init.helper_id IS NOT NULL AND init.metadata_layout_digest IS NOT NULL";

const RESOURCES: &str = "CREATE VIEW capacity_reservation_unbound_resource_attempts AS SELECT reservation_id FROM capacity_reservation_provisioning_objects WHERE create_state IN ('created','uncertain') UNION SELECT reservation_id FROM capacity_reservation_networks WHERE create_state IN ('created','uncertain') UNION SELECT reservation_id FROM capacity_reservation_storage_initialization WHERE abort_state='absent'";

const STATE: &str = "s.resources_bound=0 AND s.jit_state='not_requested' AND s.publication_state='not_requested' AND s.delivery_state='not_requested' AND s.storage_cleanup_proven=0";

pub(super) fn objects() -> Vec<(&'static str, String, String)> {
    let candidates = format!(
        "CREATE VIEW capacity_reservation_unbound_candidates AS SELECT s.reservation_id FROM capacity_reservation_provisioning_state s JOIN capacity_reservation_unbound_container_shapes c ON c.reservation_id=s.reservation_id WHERE {STATE} AND s.bootstrap_start_state IN ('started','uncertain') AND c.pair_created=1 UNION ALL SELECT s.reservation_id FROM capacity_reservation_provisioning_state s JOIN capacity_reservation_unbound_container_shapes c ON c.reservation_id=s.reservation_id JOIN capacity_reservation_unbound_initializer_ready ready ON ready.reservation_id=s.reservation_id WHERE {STATE} AND s.bootstrap_start_state='not_requested' AND c.pair_attempted=1 UNION ALL SELECT s.reservation_id FROM capacity_reservation_provisioning_state s JOIN capacity_reservation_unbound_container_shapes c ON c.reservation_id=s.reservation_id JOIN capacity_reservation_unbound_initializer_ready ready ON ready.reservation_id=s.reservation_id JOIN capacity_reservation_unbound_resource_attempts resource ON resource.reservation_id=s.reservation_id WHERE {STATE} AND s.bootstrap_start_state='not_requested' AND c.no_effects=1"
    );
    vec![
        (
            "view",
            "capacity_reservation_unbound_classifications".to_owned(),
            format!(
                "CREATE VIEW capacity_reservation_unbound_classifications(reservation_id,connection_id) AS {}",
                crate::journal::unbound_ownership::CLASSIFICATION
            ),
        ),
        (
            "view",
            "capacity_reservation_unbound_container_shapes".to_owned(),
            CONTAINERS.to_owned(),
        ),
        (
            "view",
            "capacity_reservation_unbound_initializer_ready".to_owned(),
            INITIALIZERS.to_owned(),
        ),
        (
            "view",
            "capacity_reservation_unbound_resource_attempts".to_owned(),
            RESOURCES.to_owned(),
        ),
        (
            "view",
            "capacity_reservation_unbound_candidates".to_owned(),
            candidates,
        ),
        (
            "view",
            "capacity_reservation_unbound_owners".to_owned(),
            format!(
                "CREATE VIEW capacity_reservation_unbound_owners(reservation_id,connection_id) AS {}",
                crate::journal::unbound_ownership::base_sql()
            ),
        ),
    ]
}
