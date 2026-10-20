use serde_json::{Value, json};

use super::{NpmRegistry, PublishNpm};
use crate::config::PublishConfig;

fn entry(edit: impl FnOnce(&mut Value)) -> Value {
    let mut wire = json!({
        "id": "ui",
        "root": "frontend",
        "member": "@chainargos/ui",
        "pack_script": "pack",
        "tarballs": "frontend/.portable/tarballs",
        "registry": "github"
    });
    edit(&mut wire);
    wire
}

fn problem(wire: &Value) -> String {
    let config: PublishConfig = serde_json::from_value(json!({ "npm": [wire] })).expect("config");
    config
        .validate("config.toml")
        .expect_err("invalid")
        .to_string()
}

#[test]
fn a_complete_entry_validates_and_names_its_tarball() {
    let parsed: PublishNpm = serde_json::from_value(entry(|_| {})).expect("entry");
    assert_eq!(parsed.registry, NpmRegistry::Github);
    assert_eq!(parsed.tarball_name("1.4.0"), "chainargos-ui-1.4.0.tgz");
    let config: PublishConfig =
        serde_json::from_value(json!({ "npm": [entry(|_| {})] })).expect("config");
    config.validate("config.toml").expect("valid");
    assert!(!config.is_empty());
}

#[test]
fn unknown_registries_keys_and_bad_grammar_are_rejected() {
    assert!(
        serde_json::from_value::<PublishNpm>(entry(|w| w["registry"] = json!("npmjs"))).is_err()
    );
    assert!(
        serde_json::from_value::<PublishNpm>(entry(|w| w["run"] = json!("npm publish"))).is_err()
    );
    assert!(problem(&entry(|w| w["id"] = json!("UI"))).contains("bad_artifact_id"));
    assert!(problem(&entry(|w| w["member"] = json!("ui"))).contains("bad_scoped_member"));
    assert!(problem(&entry(|w| w["pack_script"] = json!("a b"))).contains("bad_script"));
    assert!(problem(&entry(|w| w["root"] = json!("../up"))).contains("unsafe_path"));
    assert!(problem(&entry(|w| w["tarballs"] = json!("/abs"))).contains("unsafe_path"));
}

#[test]
fn ids_are_sorted_unique_and_shared_with_no_bundle() {
    let wire = |ids: &[&str]| -> PublishConfig {
        let npm: Vec<Value> = ids
            .iter()
            .map(|id| entry(|w| w["id"] = json!(id)))
            .collect();
        serde_json::from_value(json!({ "npm": npm })).expect("config")
    };
    assert!(
        wire(&["b", "a"])
            .validate("c")
            .expect_err("sorted")
            .to_string()
            .contains("must_be_sorted")
    );
    assert!(
        wire(&["a", "a"])
            .validate("c")
            .expect_err("dup")
            .to_string()
            .contains("duplicate_id")
    );
    let mixed: PublishConfig = serde_json::from_value(json!({
        "bundles": [{ "id": "ui", "paths": ["a"], "version": "1.0.0" }],
        "npm": [entry(|_| {})]
    }))
    .expect("config");
    assert!(
        mixed
            .validate("c")
            .expect_err("shared")
            .to_string()
            .contains("duplicate_id:ui")
    );
}
