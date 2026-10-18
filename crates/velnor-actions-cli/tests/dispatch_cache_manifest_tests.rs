use super::{PayloadManifest, PayloadRoot};
use serde_json::{Value, json};
use velnor_actions_contract::{canonical_json_str, workflow::QualificationCacheBackendEntry};
use velnor_actions_orchestrator::QualificationCacheBindingOutputs;

use super::super::roots::{CacheRootId, CacheRootSet};

const CACHE_KEY: &str = "velnor-qcache-v2-campaign-1-mbx_objects-k1";
const ROOT_DIGEST: &str = "b3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn canonical_manifest_round_trips_only_for_the_exact_bound_predecessor() {
    let roots = roots();
    let binding = binding();
    let manifest = PayloadManifest::for_save(
        &binding,
        &roots,
        CACHE_KEY,
        ROOT_DIGEST,
        vec![PayloadRoot {
            id: CacheRootId::MbxActions,
            present: true,
            digest: Some(ROOT_DIGEST.to_owned()),
        }],
    )
    .expect("save manifest");
    let encoded = manifest.canonical_json().expect("canonical manifest");

    assert_eq!(
        PayloadManifest::parse_for_restore(&encoded, &binding, &roots, CACHE_KEY)
            .expect("validated predecessor"),
        manifest
    );
    assert_eq!(
        PayloadManifest::parse_for_restore(&encoded, &binding, &roots, "another-key"),
        Err("qualification_cache_manifest_binding_mismatch")
    );
}

#[test]
fn manifest_rejects_stale_lane_identity_state_and_unadmitted_objects() {
    let roots = roots();
    let binding = binding();
    let manifest = PayloadManifest::for_save(
        &binding,
        &roots,
        CACHE_KEY,
        ROOT_DIGEST,
        vec![PayloadRoot {
            id: CacheRootId::MbxActions,
            present: true,
            digest: Some(ROOT_DIGEST.to_owned()),
        }],
    )
    .expect("save manifest");
    let encoded = manifest.canonical_json().expect("canonical manifest");

    let mut stale_lane = binding.clone();
    stale_lane.matrix_key = "other-lane".to_owned();
    assert_eq!(
        PayloadManifest::parse_for_restore(&encoded, &stale_lane, &roots, CACHE_KEY),
        Err("qualification_cache_manifest_binding_mismatch")
    );

    let mut stale_state = binding.clone();
    stale_state.expected_prior_state_digest = Some(format!("b3-{}", "b".repeat(64)));
    assert_eq!(
        PayloadManifest::parse_for_restore(&encoded, &stale_state, &roots, CACHE_KEY),
        Err("qualification_cache_manifest_binding_mismatch")
    );

    let mut unadmitted = binding;
    unadmitted.expected_cache = None;
    assert_eq!(
        PayloadManifest::parse_for_restore(&encoded, &unadmitted, &roots, CACHE_KEY),
        Err("qualification_cache_manifest_binding_mismatch")
    );
}

#[test]
fn manifest_rejects_unknown_noncanonical_and_malformed_root_metadata() {
    let roots = roots();
    let binding = binding();
    let manifest = PayloadManifest::for_save(
        &binding,
        &roots,
        CACHE_KEY,
        ROOT_DIGEST,
        vec![PayloadRoot {
            id: CacheRootId::MbxActions,
            present: true,
            digest: Some(ROOT_DIGEST.to_owned()),
        }],
    )
    .expect("save manifest");
    let encoded = manifest.canonical_json().expect("canonical manifest");

    assert_eq!(
        PayloadManifest::parse_for_restore(&format!("{encoded} "), &binding, &roots, CACHE_KEY),
        Err("qualification_cache_manifest_noncanonical")
    );

    let mut with_unknown: Value = serde_json::from_str(&encoded).expect("manifest JSON");
    with_unknown["unchecked"] = json!(true);
    let with_unknown = canonical_json_str(&with_unknown).expect("canonicalize test document");
    assert_eq!(
        PayloadManifest::parse_for_restore(&with_unknown, &binding, &roots, CACHE_KEY),
        Err("qualification_cache_manifest_invalid")
    );

    let mut wrong_kind: Value = serde_json::from_str(&encoded).expect("manifest JSON");
    wrong_kind["roots"][0]["kind"] = json!("file");
    let wrong_kind = canonical_json_str(&wrong_kind).expect("canonicalize test document");
    assert_eq!(
        PayloadManifest::parse_for_restore(&wrong_kind, &binding, &roots, CACHE_KEY),
        Err("qualification_cache_manifest_root_kind_invalid")
    );

    let mut malformed_digest: Value = serde_json::from_str(&encoded).expect("manifest JSON");
    malformed_digest["roots"][0]["digest"] = json!("sha256:bad");
    let malformed_digest =
        canonical_json_str(&malformed_digest).expect("canonicalize test document");
    assert_eq!(
        PayloadManifest::parse_for_restore(&malformed_digest, &binding, &roots, CACHE_KEY),
        Err("qualification_cache_manifest_digest_invalid")
    );
}

fn roots() -> CacheRootSet {
    let value = json!({
        "schema": "velnor.qualification-cache-roots/v1",
        "matrix_key": "lane-1",
        "layer": "mbx_objects",
        "roots": [{"id": "mbx_actions", "path": "/tmp/runner/velnor/mbx/actions"}]
    });
    CacheRootSet::parse(
        &canonical_json_str(&value).expect("canonical root observation"),
        "lane-1",
        velnor_actions_contract::workflow::QualificationCacheLayer::MbxObjects,
    )
    .expect("root observation")
}

fn binding() -> QualificationCacheBindingOutputs {
    QualificationCacheBindingOutputs {
        active: true,
        campaign: "campaign-1".to_owned(),
        phase: "cold".to_owned(),
        plan_id: "plan-1".to_owned(),
        run_key: "r10-a1".to_owned(),
        matrix_key: "lane-1".to_owned(),
        layer: "mbx_objects".to_owned(),
        restore_key: Some(CACHE_KEY.to_owned()),
        expected_cache: Some(QualificationCacheBackendEntry {
            id: 71,
            key: CACHE_KEY.to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            version: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
            size_bytes: 512,
        }),
        save_key: Some(CACHE_KEY.to_owned()),
        save_if_state_changes: false,
        expected_prior_state_digest: Some(ROOT_DIGEST.to_owned()),
        runtime_identity_digest: Some(format!("b3-{}", "c".repeat(64))),
    }
}
