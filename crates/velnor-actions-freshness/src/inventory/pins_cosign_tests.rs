use std::path::PathBuf;

use serde_json::{Value, json};

use super::check_cosign_asset_pins;
use crate::context::FreshnessContext;

fn repository_context() -> FreshnessContext {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    FreshnessContext::new(root, false, false)
}

fn cosign_inventory() -> Value {
    let inventory: Value =
        serde_json::from_str(include_str!("../../../../.velnor/freshness-inventory.json"))
            .expect("freshness inventory is valid JSON");
    inventory["tools"]
        .as_array()
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some("cosign"))
        })
        .cloned()
        .expect("freshness inventory contains the cosign row")
}

#[test]
fn cosign_asset_inventory_mirrors_both_linux_platform_pins() {
    let mut context = repository_context();

    check_cosign_asset_pins(&mut context, &cosign_inventory());

    assert!(context.failures.is_empty(), "{:?}", context.failures);
    for subject in [
        "tool cosign Linux x86_64 release size",
        "tool cosign Linux x86_64 release digest",
        "tool cosign Linux arm64 release size",
        "tool cosign Linux arm64 release digest",
    ] {
        assert!(
            context.output.contains(&format!("ok: local-pin {subject}")),
            "missing freshness success row for {subject}"
        );
    }
}

#[test]
fn cosign_arm64_inventory_rejects_wrong_digest_or_size() {
    for (field, value, subject) in [
        (
            "arm64_sha256",
            json!("0000000000000000000000000000000000000000000000000000000000000000"),
            "tool cosign Linux arm64 release digest",
        ),
        (
            "arm64_bytes",
            json!(136_437_760),
            "tool cosign Linux arm64 release size",
        ),
    ] {
        let mut inventory = cosign_inventory();
        inventory[field] = value;
        let mut context = repository_context();

        check_cosign_asset_pins(&mut context, &inventory);

        assert!(
            context
                .failures
                .iter()
                .any(|failure| failure.contains(subject)),
            "wrong {field} did not fail {subject}: {:?}",
            context.failures
        );
    }
}
