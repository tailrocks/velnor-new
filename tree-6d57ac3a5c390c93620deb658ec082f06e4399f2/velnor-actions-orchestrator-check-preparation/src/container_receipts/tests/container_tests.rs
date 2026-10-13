use super::*;

#[test]
fn container_presence_is_mandatory_and_non_container_is_empty() {
    assert!(validate_container_receipt(&runner(None), None).is_ok());
    assert!(validate_container_receipt(&runner(Some(profile())), None).is_err());
    assert!(validate_container_receipt(&runner(None), Some(&receipt())).is_err());
}

#[test]
fn exact_profile_and_execution_identity_are_bound() {
    let configured = runner(Some(profile()));
    let mut valid = receipt();
    assert!(validate_container_receipt(&configured, Some(&valid)).is_ok());
    valid.profile_digest = digest_b3(b"foreign profile");
    assert!(validate_container_receipt(&configured, Some(&valid)).is_err());
    let mut changed = receipt();
    changed
        .after
        .container
        .as_mut()
        .expect("container")
        .daemon
        .id = "daemon-2".into();
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());
}

#[test]
fn owned_cli_and_runtime_inventory_are_exact() {
    let configured = runner(Some(profile()));
    for mutation in 0..4 {
        let mut changed = receipt();
        match mutation {
            0 => {
                changed
                    .before
                    .container
                    .as_mut()
                    .expect("container")
                    .docker_program = PathBuf::from("/usr/bin/docker");
            }
            1 => {
                changed
                    .before
                    .container
                    .as_mut()
                    .expect("container")
                    .docker_sha256 = "b".repeat(64);
            }
            2 => changed.runtime["context_hash"] = json!("foreign"),
            _ => changed.runtime["runtime_entries"][0]["owner"] = json!(1),
        }
        assert!(
            validate_container_receipt(&configured, Some(&changed)).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn constructor_validates_before_returning_and_receipt_is_strict_json() {
    let configured = runner(Some(profile()));
    let before = CheckCapabilityProof {
        container: Some(observation()),
    };
    let after = before.clone();
    let built = container_receipt(
        &configured,
        before,
        after,
        Option::<Value>::None,
        Some(runtime()),
        Some(runtime_observation()),
        Some(runtime_observation()),
    )
    .expect("receipt")
    .expect("container");
    assert!(is_valid_digest(&built.profile_digest));
    let mut value = serde_json::to_value(built).expect("json");
    value["foreign"] = json!(true);
    assert!(serde_json::from_value::<ContainerReceipt>(value).is_err());
}

#[test]
fn orbstack_requires_the_sdk_projection_evidence() {
    let profile = orb_profile();
    assert!(validate_sdk(&profile, None, &observation(), &observation()).is_err());
}

#[test]
fn runtime_snapshot_and_root_shape_are_bound() {
    let configured = runner(Some(profile()));
    let mut changed = receipt();
    changed
        .before_runtime
        .as_mut()
        .expect("before runtime")
        .socket
        .inode = 3;
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());

    let mut changed = receipt();
    changed.runtime["socket"]["device"] = json!(99);
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());

    let mut changed = receipt();
    changed.runtime["runtime_root"] = json!({
        "path":"/tmp/runtime",
        "owner":0,
        "group":0,
        "mode":0o040_700,
        "device":1,
        "inode":3
    });
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());

    let mut changed = receipt();
    changed
        .runtime
        .as_object_mut()
        .expect("runtime")
        .remove("socket");
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());
}

#[test]
fn orbstack_runtime_root_is_required_and_bound() {
    let profile = orb_profile();
    let observed = orb_observation();
    let (mut value, runtime_observation) = orb_runtime();
    assert!(
        runtime::validate(
            &profile,
            &value,
            &observed,
            &observed,
            &runtime_observation,
            &runtime_observation,
        )
        .is_ok()
    );

    value["runtime_root"] = json!(null);
    assert!(
        runtime::validate(
            &profile,
            &value,
            &observed,
            &observed,
            &runtime_observation,
            &runtime_observation,
        )
        .is_err()
    );

    let (mut value, runtime_observation) = orb_runtime();
    value["runtime_root"]["inode"] = json!(4);
    assert!(
        runtime::validate(
            &profile,
            &value,
            &observed,
            &observed,
            &runtime_observation,
            &runtime_observation,
        )
        .is_err()
    );
}
