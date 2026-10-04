use super::*;

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn cache_version_uses_identical_original_order_and_final_evidence_path() {
    let original = format!("{PREFIX}mise\n{PREFIX}rustup\n{PREFIX}cargo/bin");
    let layout = build(&original, DIGEST, Vec::new()).expect("closed roots");
    let save = layout.transport_paths();
    let restore = layout.clone();
    assert_eq!(save.as_bytes(), restore.transport_paths().as_bytes());
    assert_eq!(
        save,
        format!(
            "{original}\n{}",
            velnor_actions_contract::cache_receipt_root(DIGEST)
        )
    );
    assert_eq!(layout.payload_roots(), ["mise", "rustup", "cargo/bin"]);
    assert_eq!(layout.payload_indices(), [0, 1, 2]);
    assert_eq!(layout.evidence_index(), 3);
    assert_eq!(layout.evidence_files(), EVIDENCE_FILES);
    // Ordered path input is the SDK cache-version boundary, separate from visible key.
    assert_ne!(
        velnor_actions_contract::compiled_source_sha256(save.as_bytes()),
        velnor_actions_contract::compiled_source_sha256(original.as_bytes())
    );
    let reordered = format!("{PREFIX}rustup\n{PREFIX}mise\n{PREFIX}cargo/bin");
    let changed = build(&reordered, DIGEST, Vec::new()).expect("distinct admitted order");
    assert_ne!(save.as_bytes(), changed.transport_paths().as_bytes());
}

#[test]
fn signed_payload_inventory_excludes_evidence_and_admits_only_closed_optional_roots() {
    let original =
        format!("{PREFIX}cargo/bin\n{PREFIX}cargo/.crates.toml\n{PREFIX}cargo/.crates2.json");
    let optional = vec!["cargo/.crates.toml".into(), "cargo/.crates2.json".into()];
    let layout = build(&original, DIGEST, optional.clone()).expect("full tool roots");
    assert_eq!(layout.optional_roots(), optional);
    assert!(
        !layout
            .payload_roots()
            .iter()
            .any(|root| root.starts_with("cache-receipts"))
    );
    assert_eq!(layout.descriptor()["evidence_index"], 3);
    assert_eq!(layout.descriptor()["payload_indices"], json!([0, 1, 2]));
    assert_eq!(layout.descriptor()["evidence_files"], json!(EVIDENCE_FILES));
    assert!(build(&original, DIGEST, vec!["unadmitted".into()]).is_err());
}

#[test]
fn forged_namespace_order_and_evidence_cycles_reject() {
    for roots in [
        "",
        "mise\n",
        "mise\nmise",
        "mise\nmise/bin",
        "../mise",
        "/mise",
        "mise//bin",
        "mise/./bin",
        "mise/../bin",
        "mise\\bin",
        "mise\r",
        "mise/*",
        "mise/[bin]",
        "mise/ bin",
        "mise ",
        "cache-receipts",
        "cache-receipts/forged",
        "${{ caller.root }}",
    ] {
        let paths = roots
            .split('\n')
            .map(|root| format!("{PREFIX}{root}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            build(&paths, DIGEST, Vec::new()).is_err(),
            "accepted {roots:?}"
        );
    }
    let paths = format!("{PREFIX}mise");
    for digest in ["", "abc", &"A".repeat(64), &"g".repeat(64)] {
        assert!(build(&paths, digest, Vec::new()).is_err());
    }
    assert!(build("/caller/path", DIGEST, Vec::new()).is_err());
}

#[test]
fn layout_is_bounded_before_runtime_or_archive_observation() {
    let paths = (0..=MAX_ROOTS)
        .map(|index| format!("{PREFIX}root-{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(build(&paths, DIGEST, Vec::new()).is_err());
    let oversized = format!("{PREFIX}{}", "a".repeat(MAX_PATH_BYTES + 1));
    assert!(build(&oversized, DIGEST, Vec::new()).is_err());
    assert!(build(&"a".repeat(MAX_TRANSPORT_BYTES + 1), DIGEST, Vec::new()).is_err());
}

#[test]
fn frozen_descriptor_cannot_replace_index_paths_or_evidence_inventory() {
    let layout = build(&format!("{PREFIX}mise"), DIGEST, Vec::new()).expect("closed layout");
    let descriptor = json!({
        "recipe_sha256": DIGEST, "allowed_roots": layout.payload_roots(),
        "optional_roots": layout.optional_roots(), "transport_layout": layout.descriptor(),
    });
    validate_frozen_descriptor(&layout, DIGEST, &descriptor).expect("exact factory projection");
    for (field, value) in [
        ("payload_indices", json!([1])),
        ("evidence_index", json!(0)),
        ("sdk_paths", json!(["/caller/mise", "/caller/evidence"])),
        (
            "evidence_files",
            json!(["manifest.json", "predicate.json", "extra.json"]),
        ),
        ("payload_roots", json!([layout.evidence_root()])),
        ("optional_roots", json!(["mise"])),
        ("schema", json!(2)),
    ] {
        let mut forged = descriptor.clone();
        forged["transport_layout"][field] = value;
        assert!(
            validate_frozen_descriptor(&layout, DIGEST, &forged).is_err(),
            "accepted {field}"
        );
    }
    for (field, value) in [
        ("recipe_sha256", json!("b".repeat(64))),
        ("allowed_roots", json!(["caller"])),
        ("optional_roots", json!(["mise"])),
    ] {
        let mut forged = descriptor.clone();
        forged[field] = value;
        assert!(validate_frozen_descriptor(&layout, DIGEST, &forged).is_err());
    }
}

#[test]
fn mbx_fixed_bundle_root_keeps_evidence_separate_and_last() {
    let original = format!("{PREFIX}mbx-export/b3-{DIGEST}/bundle");
    let layout = build(&original, DIGEST, Vec::new()).expect("bounded MBX bundle root");
    assert_eq!(layout.payload_indices(), [0]);
    assert_eq!(layout.evidence_index(), 1);
    assert_eq!(
        layout.payload_roots(),
        [format!("mbx-export/b3-{DIGEST}/bundle")]
    );
    assert_eq!(
        layout.transport_paths(),
        format!(
            "{original}\n{}",
            velnor_actions_contract::cache_receipt_root(DIGEST)
        )
    );
}
