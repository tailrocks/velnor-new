use super::{AnsibleCheck, AnsibleStackConfig, LintProfile, is_valid_collection_name};
use serde_json::{Value, json};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn base() -> Value {
    json!({
        "roots": ["ansible-configs"],
        "python_requirements": "ansible-configs/requirements.txt",
        "inventory": "ansible-configs/hosts.ini"
    })
}

fn with(extra: Value) -> Value {
    let mut wire = base();
    if let (Some(map), Value::Object(add)) = (wire.as_object_mut(), extra) {
        map.extend(add);
    }
    wire
}

fn parse(wire: Value) -> AnsibleStackConfig {
    serde_json::from_value(wire).expect("ansible stack")
}

fn problem(wire: Value) -> String {
    parse(wire)
        .validate("config.toml")
        .expect_err("invalid")
        .to_string()
}

#[test]
fn minimal_table_validates_round_trips_and_defaults() {
    let config = parse(base());
    config.validate("config.toml").expect("valid");
    assert_eq!(
        config.effective_checks(),
        [AnsibleCheck::Syntax, AnsibleCheck::Lint]
    );
    assert_eq!(config.effective_lint_profile(), LintProfile::Production);
    assert_eq!(serde_json::to_value(&config).expect("serialize"), base());
}

#[test]
fn full_table_validates() {
    parse(with(json!({
        "checks": ["syntax", "lint"],
        "lint_profile": "safety",
        "timeout_minutes": 15,
        "collections": [
            { "name": "ansible.posix", "version": "2.2.2", "sha256": SHA },
            { "name": "community.general", "version": "13.4.0", "sha256": SHA }
        ]
    })))
    .validate("config.toml")
    .expect("valid");
}

#[test]
fn unknown_keys_and_enums_are_rejected() {
    for wire in [
        with(json!({ "run": "ansible-playbook x" })),
        with(json!({ "checks": ["deploy"] })),
        with(json!({ "lint_profile": "strict" })),
        with(
            json!({ "collections": [{ "name": "a.b", "version": "1.0.0", "sha256": SHA, "url": "x" }] }),
        ),
    ] {
        assert!(serde_json::from_value::<AnsibleStackConfig>(wire).is_err());
    }
}

#[test]
fn roots_paths_and_checks_are_checked() {
    assert!(problem(with(json!({ "roots": [] }))).contains("empty_roots"));
    assert!(problem(with(json!({ "roots": ["b", "a"] }))).contains("must_be_sorted"));
    assert!(problem(with(json!({ "inventory": "../hosts.ini" }))).contains("bad_file_path"));
    assert!(problem(with(json!({ "python_requirements": "/abs.txt" }))).contains("bad_file_path"));
    assert!(problem(with(json!({ "checks": ["lint", "syntax"] }))).contains("sorted_and_unique"));
    assert!(
        problem(with(json!({ "checks": ["syntax"], "lint_profile": "min" })))
            .contains("requires_the_lint_check")
    );
    assert!(problem(with(json!({ "timeout_minutes": 361 }))).contains("timeout_minutes"));
}

#[test]
fn collections_are_exact_and_sorted() {
    let entry = |name: &str, version: &str, sha: &str| json!({ "name": name, "version": version, "sha256": sha });
    assert!(
        problem(with(
            json!({ "collections": [entry("b.x", "1.0.0", SHA), entry("a.x", "1.0.0", SHA)] })
        ))
        .contains("sorted_and_unique")
    );
    assert!(
        problem(with(
            json!({ "collections": [entry("community", "1.0.0", SHA)] })
        ))
        .contains("bad_collection_name")
    );
    assert!(
        problem(with(json!({ "collections": [entry("a.b", "1.0", SHA)] })))
            .contains("bad_collection_version")
    );
    assert!(
        problem(with(
            json!({ "collections": [entry("a.b", "1.0.0", "ABC")] })
        ))
        .contains("bad_collection_sha256")
    );
    assert!(is_valid_collection_name(
        "community.library_inventory_filtering_v1"
    ));
    assert!(!is_valid_collection_name("a.b.c") && !is_valid_collection_name("A.b"));
}
