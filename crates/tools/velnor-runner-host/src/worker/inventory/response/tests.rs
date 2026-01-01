use super::{BoundedMap, BoundedVec, BoundedVolumeListResponse, InventoryContainer};
use crate::worker::{MAX_INVENTORY_OBJECTS_PER_KIND, inventory::MAX_CONTAINER_NAMES};

#[test]
fn bounded_sequence_rejects_the_first_item_past_its_limit() {
    assert_eq!(
        serde_json::from_str::<BoundedVec<u8, 2>>("[1,2]")
            .expect("two items fit")
            .into_vec(),
        [1, 2]
    );
    assert!(serde_json::from_str::<BoundedVec<u8, 2>>("[1,2,3]").is_err());
}

#[test]
fn docker_list_object_cap_is_enforced_during_decode() {
    let mut body = String::from("[");
    for index in 0..=MAX_INVENTORY_OBJECTS_PER_KIND {
        if index > 0 {
            body.push(',');
        }
        body.push('0');
    }
    body.push(']');

    assert!(serde_json::from_str::<BoundedVec<u8, MAX_INVENTORY_OBJECTS_PER_KIND>>(&body).is_err());
}

#[test]
fn volume_envelope_requires_complete_data_and_no_warnings() {
    let empty =
        serde_json::from_str::<BoundedVolumeListResponse>(r#"{"Volumes":[],"Warnings":[]}"#)
            .expect("valid empty list");
    assert_eq!(empty.into_volumes().expect("present list").len(), 0);

    let missing = serde_json::from_str::<BoundedVolumeListResponse>(r#"{"Warnings":[]}"#)
        .expect("valid envelope syntax");
    assert!(missing.into_volumes().is_err());

    let warned = serde_json::from_str::<BoundedVolumeListResponse>(
        r#"{"Volumes":[],"Warnings":["incomplete"]}"#,
    )
    .expect("valid envelope syntax");
    assert!(warned.into_volumes().is_err());
}

#[test]
fn nested_name_and_label_collections_are_bounded_during_deserialization() {
    let names = serde_json::to_string(
        &(0..=MAX_CONTAINER_NAMES)
            .map(|_| "/runner")
            .collect::<Vec<_>>(),
    )
    .expect("names serialize");
    let container = format!(r#"{{"Names":{names}}}"#);
    assert!(serde_json::from_str::<InventoryContainer>(&container).is_err());

    let labels = serde_json::to_string(
        &(0..=super::super::MAX_LABEL_COUNT)
            .map(|index| (format!("k{index}"), "v"))
            .collect::<std::collections::HashMap<_, _>>(),
    )
    .expect("labels serialize");
    let container = format!(r#"{{"Labels":{labels}}}"#);
    assert!(serde_json::from_str::<InventoryContainer>(&container).is_err());
}

#[test]
fn duplicate_label_keys_are_not_silently_overwritten() {
    assert!(
        serde_json::from_str::<BoundedMap<String, String, 4>>(r#"{"key":"a","key":"b"}"#).is_err()
    );
}

#[test]
fn unknown_nested_docker_fields_are_ignored_without_changing_identity_fields() {
    let container = serde_json::from_str::<InventoryContainer>(
        r#"{"Id":"container-1","Names":["/runner"],"Ports":[{"PrivatePort":1}],"Mounts":[{"Source":"unused"}],"NetworkSettings":{"Networks":{"unused":{"Aliases":["x"]}}}}"#,
    )
    .expect("new Docker fields stay forward-compatible");
    assert_eq!(container.id.as_deref(), Some("container-1"));
    assert_eq!(container.names.expect("names").into_vec(), ["/runner"]);
}
