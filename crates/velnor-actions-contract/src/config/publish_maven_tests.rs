use serde_json::{Value, json};

use super::PublishMaven;
use crate::config::PublishConfig;

fn entry(edit: impl FnOnce(&mut Value)) -> Value {
    let mut wire = json!({ "id": "processor-shared-libs", "root": "backend" });
    edit(&mut wire);
    wire
}

fn problem(entries: &[Value]) -> String {
    let config: PublishConfig =
        serde_json::from_value(json!({ "maven": entries })).expect("config");
    config
        .validate("config.toml")
        .expect_err("invalid")
        .to_string()
}

#[test]
fn a_complete_entry_validates_and_names_its_version_file_and_output_dir() {
    let parsed: PublishMaven = serde_json::from_value(entry(|_| {})).expect("entry");
    assert_eq!(parsed.version_file(), "backend/gradle.properties");
    assert_eq!(parsed.output_dir(), "backend/build/maven-bundle");
    let at_root: PublishMaven =
        serde_json::from_value(entry(|w| w["root"] = json!("."))).expect("entry");
    assert_eq!(at_root.version_file(), "gradle.properties");
    assert_eq!(at_root.output_dir(), "build/maven-bundle");
    let config: PublishConfig =
        serde_json::from_value(json!({ "maven": [entry(|_| {})] })).expect("config");
    config.validate("config.toml").expect("valid");
    assert!(!config.is_empty());
}

#[test]
fn unknown_keys_and_bad_grammar_are_rejected() {
    assert!(
        serde_json::from_value::<PublishMaven>(entry(|w| w["task"] = json!(":publish"))).is_err()
    );
    assert!(problem(&[entry(|w| w["id"] = json!("Libs"))]).contains("bad_artifact_id"));
    assert!(problem(&[entry(|w| w["root"] = json!("../up"))]).contains("unsafe_path"));
    assert!(problem(&[entry(|w| w["root"] = json!("/abs"))]).contains("unsafe_path"));
}

#[test]
fn ids_are_sorted_unique_and_shared_across_publish_kinds() {
    let a = entry(|w| w["id"] = json!("a-libs"));
    let b = entry(|w| w["id"] = json!("b-libs"));
    assert!(problem(&[b.clone(), a.clone()]).contains("must_be_sorted_by_id"));
    assert!(problem(&[a.clone(), a.clone()]).contains("duplicate_id:a-libs"));
    let config: PublishConfig = serde_json::from_value(json!({
        "bundles": [{ "id": "a-libs", "paths": ["x"], "version": "1.0.0" }],
        "maven": [a],
    }))
    .expect("config");
    let text = config
        .validate("config.toml")
        .expect_err("shared id")
        .to_string();
    assert!(text.contains("duplicate_id:a-libs"), "{text}");
}

#[test]
fn the_removed_attestation_opt_out_keys_are_rejected() {
    for key in ["attest", "attest_reason"] {
        let wire = entry(|w| w[key] = json!(false));
        assert!(
            serde_json::from_value::<PublishMaven>(wire).is_err(),
            "{key}"
        );
    }
}
