//! `[stacks.gradle]` parsing and validation.

use super::*;

fn parse(toml_like: serde_json::Value) -> Result<GradleStackConfig, serde_json::Error> {
    serde_json::from_value(toml_like)
}

#[test]
fn accepts_roots_limits_and_catalog_services() {
    let config = parse(serde_json::json!({
        "roots": ["backend"],
        "timeout_minutes": 15,
        "max_workers": 4,
        "projects": { ":eth-domain": { "services": ["postgres"] } }
    }))
    .expect("parse");
    assert_eq!(config.validate("c.toml"), Ok(()));
    assert_eq!(
        config.projects[":eth-domain"].services,
        [GradleService::Postgres]
    );
}

#[test]
fn rejects_unknown_keys_services_and_bad_values() {
    for bad in [
        serde_json::json!({ "roots": ["a"], "argv": ["x"] }),
        serde_json::json!({ "roots": ["a"], "projects": { ":p": { "services": ["redis"] } } }),
        serde_json::json!({ "roots": ["a"], "projects": { ":p": { "env": {} } } }),
    ] {
        assert!(parse(bad).is_err());
    }
    for bad in [
        serde_json::json!({ "roots": [] }),
        serde_json::json!({ "roots": ["a", "a"] }),
        serde_json::json!({ "roots": ["a"], "timeout_minutes": 0 }),
        serde_json::json!({ "roots": ["a"], "timeout_minutes": 361 }),
        serde_json::json!({ "roots": ["a"], "max_workers": 0 }),
        serde_json::json!({ "roots": ["a"], "max_workers": 65 }),
        serde_json::json!({ "roots": ["a"], "projects": { "p": {} } }),
        serde_json::json!({ "roots": ["a"], "projects": { ":p;x": {} } }),
        serde_json::json!({ "roots": ["a"], "projects": { ":p": { "services": ["postgres", "postgres"] } } }),
    ] {
        let config = parse(bad).expect("shape parses");
        assert!(config.validate("c.toml").is_err());
    }
}
