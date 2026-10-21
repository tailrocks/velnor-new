use super::*;

fn facts() -> QualificationCacheWorkerOwnerFacts {
    let owner_id = "rust-demo".to_owned();
    let platform_id = "linux-x86_64".to_owned();
    let member_digest = velnor_actions_contract::digest_b3(b"complete owner member set");
    QualificationCacheWorkerOwnerFacts {
        schema: QualificationCacheWorkerOwnerFacts::SCHEMA,
        kind: QualificationCacheWorkerOwnerFacts::KIND.to_owned(),
        owner_id: owner_id.clone(),
        platform_id: platform_id.clone(),
        member_digest: member_digest.clone(),
        members: vec![QualificationCacheOwnerMemberRef {
            matrix_key: "rust-demo".to_owned(),
            stack_id: "rust".to_owned(),
            task_id: "fmt".to_owned(),
            task_digest: velnor_actions_contract::digest_b3(b"task configuration"),
            input_digest: velnor_actions_contract::digest_b3(b"task inputs"),
        }],
        layers: LAYERS
            .into_iter()
            .map(|layer| QualificationCacheWorkerLayerFacts {
                owner_id: owner_id.clone(),
                platform_id: platform_id.clone(),
                member_digest: member_digest.clone(),
                layer,
                active: false,
                identity_digest: velnor_actions_contract::digest_b3(
                    format!("owner layer {layer:?}").as_bytes(),
                ),
                runtime_identity: None,
                state_digest: None,
                restore: QualificationCacheWorkerRestoreFacts {
                    requested_key: None,
                    matched_key: None,
                    result: QualificationCacheRestoreResult::Disabled,
                },
                save: QualificationCacheWorkerSaveFacts {
                    requested_key: None,
                    action: QualificationCacheSaveActionResult::Disabled,
                },
            })
            .collect(),
    }
}

#[test]
fn canonical_worker_facts_round_trip_without_backend_claims() {
    let facts = facts();
    let bytes = canonical_json_bytes(&facts).expect("canonical worker facts");
    assert_eq!(
        QualificationCacheWorkerOwnerFacts::parse_canonical(&bytes).expect("parse facts"),
        facts
    );
}

#[test]
fn worker_facts_reject_final_receipt_backend_fields() {
    let mut value = serde_json::to_value(facts()).expect("worker facts JSON");
    value["layers"][0]["matched_cache"] = serde_json::json!({"status":"absent"});
    let bytes = velnor_actions_contract::canonical_json_bytes(&value)
        .expect("canonical backend-claiming JSON");
    assert!(QualificationCacheWorkerOwnerFacts::parse_canonical(&bytes).is_err());
}

#[test]
fn worker_facts_reject_unordered_or_inconsistent_layers() {
    let mut value = facts();
    value.layers.swap(0, 1);
    assert!(value.validate().is_err());

    let mut value = facts();
    value.layers[0].owner_id = "other-job".to_owned();
    assert!(value.validate().is_err());
}

#[test]
fn worker_facts_reject_noncanonical_json_and_unknown_owner_fields() {
    let facts = facts();
    let bytes = canonical_json_bytes(&facts).expect("canonical facts");
    let pretty = serde_json::to_vec_pretty(&facts).expect("pretty facts");
    assert!(QualificationCacheWorkerOwnerFacts::parse_canonical(&pretty).is_err());
    assert!(
        QualificationCacheWorkerOwnerFacts::parse_canonical(&bytes[..bytes.len() - 1]).is_err()
    );
}

#[test]
fn worker_facts_reject_restore_and_save_action_inconsistency() {
    let mut value = facts();
    value.layers[0].restore.result = QualificationCacheRestoreResult::Hit;
    assert!(value.validate().is_err());

    let mut value = facts();
    value.layers[0].save.action = QualificationCacheSaveActionResult::Succeeded;
    assert!(value.validate().is_err());
}

#[test]
fn worker_facts_require_contract_typed_digests() {
    let valid = facts();
    assert!(velnor_actions_contract::canonical::Digest::parse(&valid.member_digest).is_ok());
    assert!(valid.validate().is_ok());

    let bare = valid.member_digest[3..].to_owned();
    assert!(velnor_actions_contract::canonical::Digest::parse(&bare).is_err());

    let mut invalid_member = valid.clone();
    invalid_member.member_digest = bare.clone();
    assert!(invalid_member.validate().is_err());

    let mut invalid_task = valid.clone();
    invalid_task.members[0].task_digest = bare.clone();
    assert!(invalid_task.validate().is_err());

    let mut invalid_input = valid.clone();
    invalid_input.members[0].input_digest = bare.clone();
    assert!(invalid_input.validate().is_err());

    let mut invalid_identity = valid;
    invalid_identity.layers[0].identity_digest = bare;
    assert!(invalid_identity.validate().is_err());
}

#[test]
fn active_layer_state_digest_uses_contract_digest_format() {
    let mut layer = facts().layers.remove(0);
    layer.active = true;
    layer.runtime_identity = Some(
        serde_json::from_value(serde_json::json!({
            "platform": {
                "os": "linux",
                "arch": "x86_64",
                "runner_label": "ubuntu-latest",
                "runs_on": "ubuntu-latest",
                "image_identity": {"source": "hosted", "image_os": "ubuntu"},
                "target": "host"
            },
            "tools": [],
            "abi": []
        }))
        .expect("valid runtime identity"),
    );
    layer.state_digest = Some(velnor_actions_contract::digest_b3(b"state"));
    assert!(layer.validate_shape().is_ok());

    layer.state_digest = Some("a".repeat(64));
    assert!(layer.validate_shape().is_err());
}
