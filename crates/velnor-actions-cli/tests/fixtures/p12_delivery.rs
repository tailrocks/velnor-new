//! Delivery freshness rejects malformed evidence and ambiguous mirror keys.

use std::error::Error;
use std::path::Path;

use serde_json::{Value, json};

use super::p12_harness as harness;

const INVENTORY: &str = ".velnor/freshness-inventory.json";
const POLICY: &str = ".velnor/version-policy.toml";

/// Edit structured inventory while retaining unrelated fixture evidence.
pub(super) fn edit_inventory(
    dir: &Path,
    change: impl FnOnce(&mut Value),
) -> Result<(), Box<dyn Error>> {
    let path = dir.join(INVENTORY);
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    change(&mut value);
    std::fs::write(path, serde_json::to_string(&value)?)?;
    Ok(())
}

/// A duplicate valid entry must fail even when all pin values agree.
fn assert_duplicate(collection: &str, key: &str) -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-duplicate")?;
    edit_inventory(&fixture.dir, |inventory| {
        let entries = inventory[collection].as_array_mut().expect("fixture list");
        entries.push(entries[0].clone());
    })?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, key);
    assert!(harness::has_fail(&run, "duplicate"), "{}", run.stdout);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn duplicate_existing_tool_is_rejected() -> Result<(), Box<dyn Error>> {
    assert_duplicate("tools", "mise")
}

#[test]
fn duplicate_existing_action_is_rejected() -> Result<(), Box<dyn Error>> {
    assert_duplicate("actions", "jdx/mise-action")
}

#[test]
fn delivery_collection_and_entries_fail_with_structured_rows() -> Result<(), Box<dyn Error>> {
    let passing = harness::passing("p12-delivery-valid")?;
    harness::assert_clean(&harness::run_script(&passing.dir, &[])?);
    harness::cleanup(&passing);
    for malformed in [Value::Null, json!({}), json!("invalid"), json!([null])] {
        let fixture = harness::passing("p12-delivery-shape")?;
        edit_inventory(&fixture.dir, |inventory| {
            inventory["delivery_tools"] = malformed;
        })?;
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(&run, "inventory-shape");
        assert!(harness::has_fail(&run, "delivery tools"), "{}", run.stdout);
        assert!(!run.stderr.contains("Traceback"), "{}", run.stderr);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn missing_delivery_policy_table_is_rejected() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-delivery-policy-missing")?;
    let path = fixture.dir.join(POLICY);
    let policy = std::fs::read_to_string(&path)?;
    let start = policy
        .find("[delivery-tools]")
        .ok_or("fixture delivery table")?;
    std::fs::write(path, &policy[..start])?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "delivery tools");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn malformed_delivery_policy_table_has_structured_failure() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-delivery-policy-shape")?;
    let path = fixture.dir.join(POLICY);
    let policy = std::fs::read_to_string(&path)?;
    let start = policy
        .find("[delivery-tools]")
        .ok_or("fixture delivery table")?;
    std::fs::write(path, format!("delivery-tools = []\n{}", &policy[..start]))?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "must be a table");
    assert!(!run.stderr.contains("Traceback"), "{}", run.stderr);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn delivery_image_requires_matching_digest_qualification() -> Result<(), Box<dyn Error>> {
    for qualification in [None, Some(format!("sha256:{}", "0".repeat(64)))] {
        let fixture = harness::passing("p12-delivery-digest")?;
        edit_inventory(&fixture.dir, |inventory| {
            let entries = inventory["delivery_tools"]
                .as_array_mut()
                .expect("fixture list");
            for entry in entries
                .iter_mut()
                .filter(|entry| entry.get("image_digest").is_some())
            {
                let object = entry.as_object_mut().expect("fixture delivery tool");
                if let Some(ref digest) = qualification {
                    assert!(
                        object
                            .insert("qualified_image_digest".into(), json!(digest))
                            .is_some()
                    );
                } else {
                    assert!(object.remove("qualified_image_digest").is_some());
                }
            }
        })?;
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(&run, "delivery image digest has no matching qualification");
        assert!(harness::has_fail(&run, "buildkit"), "{}", run.stdout);
        assert!(harness::has_fail(&run, "sbom-scanner"), "{}", run.stdout);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn homebrew_source_urls_bind_the_immutable_commit() -> Result<(), Box<dyn Error>> {
    for name in ["homebrew-source", "homebrew-portable-ruby"] {
        let fixture = harness::passing("p12-native-sources")?;
        edit_inventory(&fixture.dir, |inventory| {
            let entries = inventory["native_authorities"]
                .as_array_mut()
                .expect("fixture native authorities");
            let entry = entries
                .iter_mut()
                .find(|entry| entry["name"] == name)
                .expect("fixture authority");
            entry["sources"][0] = json!("https://github.com/Homebrew/brew/tree/main");
        })?;
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(&run, "sources do not match immutable compiled authority");
        assert!(harness::has_fail(&run, name), "{}", run.stdout);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn homebrew_compiled_and_policy_pins_must_match() -> Result<(), Box<dyn Error>> {
    for (path, old, new, failure) in [
        (
            POLICY,
            "homebrew-version = \"7.0.7\"",
            "homebrew-version = \"7.0.8\"",
            "native policy/inventory mismatch",
        ),
        (
            "crates/velnor-actions-mise/src/catalog_homebrew.rs",
            "VERSION: &str = \"7.0.7\"",
            "VERSION: &str = \"7.0.8\"",
            "native homebrew-source homebrew-version",
        ),
        (
            "crates/velnor-actions-mise/src/catalog_homebrew.rs",
            "https://github.com/Homebrew/brew/tree/8e858db5584704dcd469b8e826228c0d5a5a94f6",
            "https://github.com/Homebrew/brew/tree/main",
            "source URL does not bind source commit",
        ),
    ] {
        let fixture = harness::passing("p12-native-pin-mirror")?;
        harness::mutate(&fixture.dir, path, old, new)?;
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(&run, failure);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn homebrew_authorities_reject_missing_duplicate_and_unknown_fields() -> Result<(), Box<dyn Error>>
{
    for mutation in ["missing", "duplicate", "unknown", "current-claim", "scope"] {
        let fixture = harness::passing("p12-native-shape")?;
        edit_inventory(&fixture.dir, |inventory| {
            let entries = inventory["native_authorities"]
                .as_array_mut()
                .expect("fixture native authorities");
            match mutation {
                "missing" => {
                    assert!(entries.pop().is_some());
                }
                "duplicate" => entries.push(entries[0].clone()),
                "unknown" => entries[0]["unexpected"] = json!(true),
                "current-claim" => entries[0]["status"] = json!("current"),
                "scope" => entries[0]["scope"] = json!("runtime-qualified"),
                _ => unreachable!("closed mutation table"),
            }
        })?;
        let run = harness::run_script(&fixture.dir, &[])?;
        let failure = match mutation {
            "missing" => "inventory row missing",
            "scope" => "invalid immutable authority scope or evidence kind",
            _ => "unknown, duplicate, or invalid native fields",
        };
        harness::assert_fail(&run, failure);
        assert!(!run.stderr.contains("Traceback"), "{}", run.stderr);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn homebrew_evidence_rejects_stale_and_future_timestamps() -> Result<(), Box<dyn Error>> {
    for offset in [-30, 2] {
        let fixture = harness::passing("p12-native-time")?;
        let timestamp = format!("{}T00:00:00Z", harness::days_iso(offset)?);
        edit_inventory(&fixture.dir, |inventory| {
            for entry in inventory["native_authorities"]
                .as_array_mut()
                .expect("fixture native authorities")
            {
                entry["checked_at"] = json!(timestamp);
            }
        })?;
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(
            &run,
            "missing, malformed, future, or stale evidence timestamp",
        );
        assert!(harness::has_fail(&run, "homebrew-source"), "{}", run.stdout);
        assert!(
            harness::has_fail(&run, "homebrew-portable-ruby"),
            "{}",
            run.stdout
        );
        harness::cleanup(&fixture);
    }
    Ok(())
}
