use super::{DbtCheck, DbtStackConfig, is_inert_env_value, is_valid_env_name};
use serde_json::{Value, json};

fn parse(wire: Value) -> DbtStackConfig {
    serde_json::from_value(wire).expect("dbt stack")
}

fn problem(wire: Value) -> String {
    parse(wire)
        .validate("config.toml")
        .expect_err("invalid")
        .to_string()
}

#[test]
fn minimal_projects_validate_round_trip_and_default_to_parse() {
    let wire = json!({ "projects": ["chainargos-dbt"] });
    let config = parse(wire.clone());
    config.validate("config.toml").expect("valid");
    assert_eq!(config.effective_checks(), [DbtCheck::Parse]);
    assert_eq!(serde_json::to_value(&config).expect("serialize"), wire);
}

#[test]
fn full_table_validates() {
    parse(json!({
        "projects": [".", "other"],
        "services": ["postgres"],
        "env": { "DBT_HOST": "127.0.0.1", "DBT_PASSWORD": "velnor", "DBT_PROFILES_DIR": "." },
        "checks": ["parse", "compile"],
        "timeout_minutes": 20
    }))
    .validate("config.toml")
    .expect("valid");
}

#[test]
fn unknown_keys_and_unknown_enums_are_rejected() {
    for wire in [
        json!({ "projects": ["."], "run": "dbt run" }),
        json!({ "projects": ["."], "checks": ["build"] }),
        json!({ "projects": ["."], "services": ["mysql"] }),
    ] {
        assert!(serde_json::from_value::<DbtStackConfig>(wire).is_err());
    }
}

#[test]
fn projects_must_be_sorted_unique_and_repository_relative() {
    assert!(problem(json!({ "projects": [] })).contains("empty_projects"));
    assert!(problem(json!({ "projects": ["b", "a"] })).contains("must_be_sorted"));
    assert!(problem(json!({ "projects": ["a", "a"] })).contains("duplicate_project"));
    assert!(problem(json!({ "projects": ["../x"] })).contains("bad_project"));
}

#[test]
fn compile_needs_a_service_and_lists_are_sorted() {
    assert!(
        problem(json!({ "projects": ["."], "checks": ["compile"] }))
            .contains("compile_requires_a_service")
    );
    assert!(
        problem(
            json!({ "projects": ["."], "checks": ["compile", "parse"], "services": ["postgres"] })
        )
        .contains("stacks.dbt.checks")
    );
    assert!(
        problem(json!({ "projects": ["."], "timeout_minutes": 0 })).contains("timeout_minutes")
    );
}

#[test]
fn env_names_and_values_are_inert() {
    assert!(is_valid_env_name("DBT_HOST") && !is_valid_env_name("HOME"));
    assert!(!is_valid_env_name("DBT_") && !is_valid_env_name("DBT_host"));
    assert!(is_inert_env_value("127.0.0.1") && !is_inert_env_value("${{ secrets.X }}"));
    assert!(!is_inert_env_value("a b") && !is_inert_env_value("$(id)"));
    assert!(problem(json!({ "projects": ["."], "env": { "PATH": "x" } })).contains("bad_env_name"));
    assert!(
        problem(json!({ "projects": ["."], "env": { "DBT_PASSWORD": "a;b" } }))
            .contains("bad_env_value")
    );
}
