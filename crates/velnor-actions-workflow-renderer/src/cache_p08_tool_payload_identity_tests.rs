use super::{ToolsCachePayload, inputs, inputs_with_source, payload, pinned_mise};
use crate::setup::MiseSetup;

#[test]
fn static_identity_canonicalizes_inputs_and_binds_runner_selector() {
    let setup = pinned_mise();
    let specs_a = vec!["rust@1.98.1".to_owned(), "shellcheck@0.11.0".to_owned()];
    let specs_b = vec!["shellcheck@0.11.0".to_owned(), "rust@1.98.1".to_owned()];
    let components_a = vec!["clippy".to_owned(), "rustfmt".to_owned()];
    let components_b = vec!["rustfmt".to_owned(), "clippy".to_owned()];
    let first = payload(inputs(
        "ubuntu-26.04",
        &setup,
        &specs_a,
        Some("1.98.1"),
        &components_a,
    ));
    let reordered = payload(inputs(
        "ubuntu-26.04",
        &setup,
        &specs_b,
        Some("1.98.1"),
        &components_b,
    ));
    let image_source_sha = "b".repeat(40);
    let scale_set = payload(inputs_with_source(
        "scale-set:velnor+ubuntu-26.04-scale-set",
        &setup,
        &specs_a,
        Some("1.98.1"),
        &components_a,
        Some(&image_source_sha),
    ));
    assert_eq!(first.static_digest(), reordered.static_digest());
    assert_ne!(first.static_digest(), scale_set.static_digest());
    assert_eq!(first.static_digest().len(), 64);
    assert!(
        first
            .static_digest()
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
}

#[test]
fn scale_set_cache_identity_requires_and_hashes_the_validated_image_source() {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned()];
    let scale_set = "scale-set:velnor+ubuntu-26.04-scale-set";
    let missing = payload(inputs(scale_set, &setup, &specs, Some("1.98.1"), &[]));
    assert!(!missing.runtime_identity_supported());

    let source_a = "a".repeat(40);
    let source_b = "b".repeat(40);
    let first = payload(inputs_with_source(
        scale_set,
        &setup,
        &specs,
        Some("1.98.1"),
        &[],
        Some(&source_a),
    ));
    let second = payload(inputs_with_source(
        scale_set,
        &setup,
        &specs,
        Some("1.98.1"),
        &[],
        Some(&source_b),
    ));
    assert_ne!(first.static_digest(), second.static_digest());

    assert!(
        ToolsCachePayload::new(inputs_with_source(
            scale_set,
            &setup,
            &specs,
            Some("1.98.1"),
            &[],
            Some(&"A".repeat(40)),
        ))
        .is_err()
    );
    assert!(
        ToolsCachePayload::new(inputs_with_source(
            "ubuntu-26.04",
            &setup,
            &specs,
            Some("1.98.1"),
            &[],
            Some(&source_a),
        ))
        .is_err()
    );
}

#[test]
fn runtime_identity_is_required_and_changes_the_final_key() {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned()];
    let payload = payload(inputs("ubuntu-26.04", &setup, &specs, Some("1.98.1"), &[]));
    let first_identity = "a".repeat(64);
    let second_identity = "b".repeat(64);
    assert_ne!(
        payload
            .key_for_runtime_identity(&first_identity)
            .expect("first identity"),
        payload
            .key_for_runtime_identity(&second_identity)
            .expect("second identity")
    );
    assert!(payload.key_for_runtime_identity("latest").is_err());
    assert!(
        payload
            .key_for_runtime_identity("A".repeat(64).as_str())
            .is_err()
    );
    assert_eq!(
        payload.key_expression(),
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION
    );
    assert!(crate::cache_p08::is_v2_cache_key_expression(
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION
    ));
    assert!(!crate::cache_p08::is_v2_cache_key_expression(
        "mise-tools-v2-static-digest-${{steps.v2.outputs.identity}}"
    ));
}

#[test]
fn payload_rejects_untyped_or_mismatched_tool_inputs() {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned()];
    let components = vec!["clippy".to_owned()];
    assert!(ToolsCachePayload::new(inputs("ubuntu-26.04", &setup, &[], None, &[])).is_err());
    assert!(
        ToolsCachePayload::new(inputs(
            "ubuntu-latest",
            &setup,
            &specs,
            Some("1.98.1"),
            &components
        ))
        .is_err()
    );
    assert!(
        ToolsCachePayload::new(inputs(
            "ubuntu-26.04",
            &setup,
            &specs,
            Some("1.98.2"),
            &components
        ))
        .is_err()
    );
    assert!(
        ToolsCachePayload::new(inputs("ubuntu-26.04", &setup, &specs, None, &components)).is_err()
    );
    assert!(
        ToolsCachePayload::new(inputs(
            "ubuntu-26.04",
            &setup,
            &specs,
            Some("1.98.1"),
            &["cargo".to_owned()]
        ))
        .is_err()
    );
    let invalid_setup = MiseSetup {
        version: "latest".to_owned(),
        ..setup.clone()
    };
    assert!(
        ToolsCachePayload::new(inputs(
            "ubuntu-26.04",
            &invalid_setup,
            &specs,
            Some("1.98.1"),
            &components
        ))
        .is_err()
    );
}

#[test]
fn payload_key_is_namespaced_to_static_payload_and_runtime_identity() {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned()];
    let components = vec!["clippy".to_owned()];
    let base = payload(inputs(
        "ubuntu-26.04",
        &setup,
        &specs,
        Some("1.98.1"),
        &components,
    ));
    let changed_action_setup = MiseSetup {
        uses: "jdx/mise-action@0000000000000000000000000000000000000000".to_owned(),
        ..setup.clone()
    };
    let changed_action = ToolsCachePayload::new(inputs(
        "ubuntu-26.04",
        &changed_action_setup,
        &specs,
        Some("1.98.1"),
        &components,
    ))
    .expect("different valid action SHA");
    let changed_binary_setup = MiseSetup {
        sha256: "b".repeat(64),
        ..setup.clone()
    };
    let changed_binary = ToolsCachePayload::new(inputs(
        "ubuntu-26.04",
        &changed_binary_setup,
        &specs,
        Some("1.98.1"),
        &components,
    ))
    .expect("different valid binary SHA");
    assert_ne!(base.static_digest(), changed_action.static_digest());
    assert_ne!(base.static_digest(), changed_binary.static_digest());
    let runtime = "c".repeat(64);
    assert!(
        base.key_for_runtime_identity(&runtime)
            .expect("key")
            .starts_with("mise-tools-v2-payload-v3-")
    );
}
