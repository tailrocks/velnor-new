use std::collections::BTreeMap;
use std::time::Duration;

use tokio::time::Instant;

use super::response::{BoundedVolumeListResponse, InventoryContainer, InventoryNetwork};
use super::{
    MAX_INVENTORY_OBJECTS_PER_KIND, OwnedDockerResourceKind, finish_inventory_until,
    inventory_from_lists,
};
use crate::{HostError, worker_volume_names};

fn container(id: &str, names: &[&str], labels: &serde_json::Value) -> InventoryContainer {
    serde_json::from_value(serde_json::json!({
        "Id": id,
        "Names": names,
        "Labels": labels
    }))
    .expect("container fixture")
}

fn network(id: &str, name: &str, labels: &serde_json::Value) -> InventoryNetwork {
    serde_json::from_value(serde_json::json!({
        "Id": id,
        "Name": name,
        "Labels": labels
    }))
    .expect("network fixture")
}

fn volume_list(name: &str, labels: &serde_json::Value) -> BoundedVolumeListResponse {
    serde_json::from_value(serde_json::json!({
        "Volumes": [{"Name": name, "Labels": labels}],
        "Warnings": []
    }))
    .expect("volume fixture")
}

fn empty_volume_list() -> BoundedVolumeListResponse {
    serde_json::from_value(serde_json::json!({"Volumes": [], "Warnings": []}))
        .expect("empty volume fixture")
}

fn labels(worker: &str, role: &str) -> serde_json::Value {
    serde_json::json!({"velnor.worker": worker, "velnor.role": role})
}

#[test]
fn inventory_preserves_owned_identity_labels_and_skips_unowned_objects() {
    let mut runner_labels = labels("worker-a", "runner");
    runner_labels["velnor.volume"] = serde_json::json!("worker-a");
    runner_labels["velnor.generation"] = serde_json::json!("generation-7");
    let containers = vec![
        container("container-1", &["/runner", "/legacy-alias"], &runner_labels),
        container("unowned-without-id", &[], &serde_json::json!({})),
    ];
    let networks = vec![network(
        "network-1",
        "outer-worker-a",
        &labels("worker-a", "outer-network"),
    )];
    let volumes = volume_list("worker-a-work", &labels("worker-a", "work"));

    let inventory =
        inventory_from_lists(containers, networks, volumes).expect("complete inventory");
    assert_eq!(inventory.len(), 3);
    assert_eq!(inventory[0].kind, OwnedDockerResourceKind::Container);
    assert_eq!(inventory[0].id_or_name, "container-1");
    assert_eq!(inventory[0].names, ["/legacy-alias", "/runner"]);
    assert_eq!(inventory[0].worker, "worker-a");
    assert_eq!(inventory[0].role, "runner");
    assert_eq!(
        inventory[0].labels.get("velnor.generation"),
        Some(&"generation-7".to_owned())
    );
    assert_eq!(inventory[1].kind, OwnedDockerResourceKind::Network);
    assert_eq!(inventory[1].id_or_name, "network-1");
    assert_eq!(inventory[2].kind, OwnedDockerResourceKind::Volume);
    assert_eq!(inventory[2].id_or_name, "worker-a-work");
}

#[test]
fn malformed_or_ambiguous_velnor_objects_fail_the_whole_snapshot() {
    let missing_worker = container(
        "container-1",
        &["/runner"],
        &serde_json::json!({"velnor.role": "runner"}),
    );
    assert_eq!(
        inventory_from_lists(vec![missing_worker], vec![], empty_volume_list()),
        Err(HostError::Docker)
    );

    let mismatched_volume = container(
        "container-1",
        &["/runner"],
        &serde_json::json!({
            "velnor.worker": "worker-a",
            "velnor.role": "runner",
            "velnor.volume": "worker-b"
        }),
    );
    assert_eq!(
        inventory_from_lists(vec![mismatched_volume], vec![], empty_volume_list()),
        Err(HostError::Docker)
    );

    let duplicate_name = vec![
        container("container-1", &["/runner"], &labels("worker-a", "runner")),
        container("container-2", &["/runner"], &labels("worker-b", "runner")),
    ];
    assert_eq!(
        inventory_from_lists(duplicate_name, vec![], empty_volume_list()),
        Err(HostError::Docker)
    );
}

#[test]
fn missing_volume_data_and_warnings_are_not_treated_as_empty() {
    let missing = serde_json::from_str::<BoundedVolumeListResponse>(r#"{"Warnings":[]}"#)
        .expect("valid envelope syntax");
    assert_eq!(
        inventory_from_lists(vec![], vec![], missing),
        Err(HostError::Docker)
    );
    let warned = serde_json::from_str::<BoundedVolumeListResponse>(
        r#"{"Volumes":[],"Warnings":["partial volume list"]}"#,
    )
    .expect("valid envelope syntax");
    assert_eq!(
        inventory_from_lists(vec![], vec![], warned),
        Err(HostError::Docker)
    );
}

#[test]
fn oversize_lists_fail_instead_of_returning_a_partial_snapshot() {
    let containers = vec![InventoryContainer::default(); MAX_INVENTORY_OBJECTS_PER_KIND + 1];
    assert_eq!(
        inventory_from_lists(containers, vec![], empty_volume_list()),
        Err(HostError::Docker)
    );
}

#[test]
fn worker_volume_catalog_preserves_role_order_and_validates_identity() {
    let expected = vec![
        "worker-a".to_owned(),
        "worker-a-work".to_owned(),
        "worker-a-externals".to_owned(),
        "worker-a-docker".to_owned(),
        "worker-a-home".to_owned(),
        "worker-a-tmp".to_owned(),
    ];
    let observed = worker_volume_names("worker-a").expect("valid worker");
    assert_eq!(observed, expected);
    assert_eq!(
        worker_volume_names("../worker"),
        Err(HostError::ForbiddenMount)
    );
}

#[test]
fn resource_inventory_dto_keeps_label_order_deterministic() {
    let observed = inventory_from_lists(
        vec![container(
            "container-1",
            &["/runner"],
            &serde_json::json!({
                "zeta": "keep",
                "velnor.role": "runner",
                "velnor.worker": "worker-a"
            }),
        )],
        vec![],
        empty_volume_list(),
    )
    .expect("complete inventory");
    assert_eq!(
        observed[0].labels,
        BTreeMap::from([
            ("velnor.role".to_owned(), "runner".to_owned()),
            ("velnor.worker".to_owned(), "worker-a".to_owned()),
            ("zeta".to_owned(), "keep".to_owned()),
        ])
    );
}

#[test]
fn irrelevant_docker_fields_are_ignored_without_materializing_nested_collections() {
    let container = serde_json::from_str::<InventoryContainer>(
        r#"{"Id":"container-1","Names":["/runner"],"Labels":{},"Ports":[{"PrivatePort":1}],"Mounts":[{"Source":"unused"}],"NetworkSettings":{"Networks":{"x":{"Aliases":["one"]}}}}"#,
    )
    .expect("unknown API fields are ignored");
    assert_eq!(container.id.as_deref(), Some("container-1"));
    assert_eq!(container.names.expect("names").into_vec(), ["/runner"]);
}

#[test]
fn late_synchronous_inventory_finalization_is_not_reported_as_success() {
    let deadline = Instant::now() + Duration::from_millis(10);
    let result = finish_inventory_until(deadline, || {
        std::thread::sleep(Duration::from_millis(30));
        Vec::<()>::new()
    });
    assert_eq!(result, Err(HostError::Docker));

    let expired = Instant::now() - Duration::from_millis(1);
    let called = std::cell::Cell::new(false);
    let result = finish_inventory_until(expired, || {
        called.set(true);
        Vec::<()>::new()
    });
    assert_eq!(result, Err(HostError::Docker));
    assert!(!called.get());
}
