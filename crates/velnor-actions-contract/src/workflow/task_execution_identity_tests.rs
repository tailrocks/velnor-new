//! Mandatory dimensions refuse malformed wire values and drifted proofs.

use super::*;

fn identity() -> TaskExecutionIdentity {
    let digest = crate::digest_b3(b"dimension");
    TaskExecutionIdentity::new(&digest, &digest, &digest, &digest, "default").expect("identity")
}

#[test]
fn equality_matches_all_five_validated_dimensions() {
    let identity = identity();
    assert_eq!(identity, identity.clone());
    let raw = serde_json::to_value(&identity).expect("json");
    let roundtrip: TaskExecutionIdentity = serde_json::from_value(raw.clone()).expect("identity");
    assert_eq!(identity, roundtrip);
    for field in [
        "graph_digest",
        "toolchain_id",
        "mbx_digest",
        "platform_id",
        "profile",
    ] {
        let mut changed = raw.clone();
        changed[field] = serde_json::Value::String(if field == "profile" {
            "release".to_owned()
        } else {
            crate::digest_b3(b"changed dimension")
        });
        let changed: TaskExecutionIdentity = serde_json::from_value(changed).expect("valid drift");
        assert_ne!(identity, changed, "{field}");
    }
}

#[test]
fn every_dimension_is_mandatory_and_validated() {
    let raw = serde_json::to_value(identity()).expect("json");
    for field in [
        "graph_digest",
        "toolchain_id",
        "mbx_digest",
        "platform_id",
        "profile",
    ] {
        let mut missing = raw.clone();
        missing.as_object_mut().expect("object").remove(field);
        assert!(
            serde_json::from_value::<TaskExecutionIdentity>(missing).is_err(),
            "{field}"
        );
        let mut invalid = raw.clone();
        invalid[field] = serde_json::Value::String(String::new());
        assert!(
            serde_json::from_value::<TaskExecutionIdentity>(invalid).is_err(),
            "{field}"
        );
    }
    let mut forged = raw;
    forged["proof_run_id"] = 7.into();
    assert!(serde_json::from_value::<TaskExecutionIdentity>(forged).is_err());
}

#[test]
fn proof_matches_every_planned_dimension() {
    let identity = identity();
    let digest = crate::digest_b3(b"task");
    let proof = ManifestTaskProof::new(
        "stack/rust/demo/clippy/default",
        &digest,
        &digest,
        identity.graph_digest(),
        identity.toolchain_id(),
        identity.mbx_digest(),
        identity.platform_id(),
        identity.profile(),
        7,
    )
    .expect("proof");
    assert!(identity.matches_proof(&proof));
    for field in [
        "graph_digest",
        "toolchain_id",
        "mbx_digest",
        "platform_id",
        "profile",
    ] {
        let mut raw = serde_json::to_value(&proof).expect("json");
        raw[field] = serde_json::Value::String(if field == "profile" {
            "release".to_owned()
        } else {
            crate::digest_b3(b"different")
        });
        let drifted: ManifestTaskProof = serde_json::from_value(raw).expect("valid drift");
        assert!(!identity.matches_proof(&drifted), "{field}");
    }
}
