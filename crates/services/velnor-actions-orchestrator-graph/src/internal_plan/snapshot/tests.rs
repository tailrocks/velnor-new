use super::*;

#[test]
fn canonical_bytes_ignore_key_order() {
    let left = serde_json::json!({"b": 1, "a": [1, 2]});
    let right = serde_json::json!({"a": [1, 2], "b": 1});
    assert_eq!(
        canonical_digest(&left).expect("digest"),
        canonical_digest(&right).expect("digest")
    );
    assert!(parse_canonical_json(r#"{"a": 1, "a": 2}"#).is_err());
    assert!(parse_canonical_json(r#"{"a": 1}"#).is_ok());
}

#[test]
fn triples_map_to_release_targets() {
    assert_eq!(
        map_release_triple("x86_64", "linux"),
        "x86_64-unknown-linux-gnu"
    );
    assert_eq!(map_release_triple("riscv64", "linux"), "riscv64-linux");
    assert!(!UNRESOLVED_GENERATOR_SHA.bytes().all(|b| b == b'0'));
}

#[test]
fn platform_images_and_versions_flip() {
    let linux = platform_id_for("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("platform");
    let older = platform_id_for("ubuntu-24.04", "x86_64-unknown-linux-gnu").expect("platform");
    assert_ne!(linux, older);
    assert!(velnor_actions_contract::validate_digest(&linux).is_ok());
    assert!(check_canonical_version(2).is_ok());
    assert!(check_canonical_version(0).is_err());
    assert!(check_canonical_version(1).is_err());
}

#[test]
fn platform_image_evidence_is_unobserved_not_label_split() {
    for label in ["ubuntu-26.04", "ubuntu-24.04", "self-hosted"] {
        let inputs = platform_inputs_for(label, "x86_64-unknown-linux-gnu").expect("platform");
        assert_eq!(inputs.runs_on, label);
        assert_eq!(inputs.image_os, UNOBSERVED_IMAGE_VALUE, "{label}");
        assert_eq!(inputs.image_version, UNOBSERVED_IMAGE_VALUE, "{label}");
    }
    let fabricated = PlatformInputs {
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        image_os: "ubuntu".to_owned(),
        image_version: "26.04".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
    };
    let honest = platform_id_for("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("id");
    let fake = platform_id(&fabricated).expect("id");
    assert_ne!(honest, fake);
}

#[test]
fn unknown_targets_reject_instead_of_taking_host() {
    for target in [
        "host",
        "riscv64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "",
    ] {
        let err = platform_id_for("ubuntu-26.04", target).expect_err("target");
        assert!(err.to_string().contains("unsupported_target"), "{err}");
    }
    let mac = platform_id_for("ubuntu-26.04", "aarch64-apple-darwin").expect("mac");
    assert!(velnor_actions_contract::validate_digest(&mac).is_ok());
}
