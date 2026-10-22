use super::{BunStackConfig, is_valid_member_name, is_valid_script_name};
use serde_json::{Value, json};

fn parse(wire: Value) -> BunStackConfig {
    serde_json::from_value(wire).expect("bun stack")
}

fn problem(wire: Value) -> String {
    parse(wire)
        .validate("config.toml")
        .expect_err("invalid")
        .to_string()
}

#[test]
fn minimal_roots_validate_and_round_trip() {
    let wire = json!({ "roots": ["."] });
    let config = parse(wire.clone());
    config.validate("config.toml").expect("valid");
    assert!(config.scripts.is_empty() && config.members.is_empty());
    assert_eq!(serde_json::to_value(&config).expect("serialize"), wire);
}

#[test]
fn full_table_validates() {
    let config = parse(json!({
        "roots": ["frontend", "tools"],
        "timeout_minutes": 20,
        "scripts": ["typecheck", "lint", "test", "build"],
        "members": {
            "@chainargos/ui": { "scripts": ["typecheck", "test"], "browsers": ["chromium", "webkit"] }
        },
        "inputs": [{
            "globs": ["frontend/packages/*/src/**"],
            "members": ["chainargos-docs"],
            "complete": true
        }]
    }));
    config.validate("config.toml").expect("valid");
}

#[test]
fn unknown_keys_and_shell_fields_are_rejected() {
    for wire in [
        json!({ "roots": ["."], "run": "bun test" }),
        json!({ "roots": ["."], "members": { "ui": { "env": { "A": "b" } } } }),
        json!({ "roots": ["."], "members": { "ui": { "browsers": ["opera"] } } }),
    ] {
        assert!(serde_json::from_value::<BunStackConfig>(wire).is_err());
    }
}

#[test]
fn roots_are_required_sorted_unique_and_repository_relative() {
    assert!(problem(json!({ "roots": [] })).contains("empty_roots"));
    assert!(problem(json!({ "roots": ["b", "a"] })).contains("must_be_sorted"));
    assert!(problem(json!({ "roots": ["a", "a"] })).contains("duplicate_root"));
    assert!(problem(json!({ "roots": ["/abs"] })).contains("bad_root"));
    assert!(problem(json!({ "roots": ["a/../b"] })).contains("bad_root"));
}

#[test]
fn timeout_scripts_and_browsers_are_bounded() {
    let with = |extra: Value| {
        let mut wire = json!({ "roots": ["."] });
        wire.as_object_mut()
            .expect("object")
            .extend(extra.as_object().expect("object").clone());
        problem(wire)
    };
    assert!(with(json!({ "timeout_minutes": 0 })).contains("timeout_minutes"));
    assert!(with(json!({ "timeout_minutes": 361 })).contains("timeout_minutes"));
    assert!(with(json!({ "scripts": ["a b"] })).contains("bad_script_name"));
    assert!(with(json!({ "scripts": ["$(id)"] })).contains("bad_script_name"));
    assert!(with(json!({ "scripts": ["test", "test"] })).contains("duplicate_script"));
    assert!(
        with(json!({ "members": { "ui": { "browsers": ["webkit", "chromium"] } } }))
            .contains("sorted_and_unique")
    );
}

#[test]
fn inputs_need_globs_known_member_names_and_no_duplicates() {
    let rule = |globs: Value, members: Value| {
        problem(json!({ "roots": ["."], "inputs": [{ "globs": globs, "members": members }] }))
    };
    assert!(rule(json!([]), json!(["ui"])).contains("empty_globs"));
    assert!(rule(json!(["a/**"]), json!(["ui", "ui"])).contains("bad_or_duplicate_member"));
    assert!(rule(json!(["../escape/**"]), json!(["ui"])).contains("bad_glob"));
}

#[test]
fn name_grammars_admit_nothing_a_shell_could_evaluate() {
    for name in ["@chainargos/ui", "chainargos-docs", "platform", "a.b_c-d"] {
        assert!(is_valid_member_name(name), "{name}");
    }
    for name in [
        "", "@x", "@/x", "UI", "a b", "a;b", "$(id)", "../x", "-x", "@a/-b",
    ] {
        assert!(!is_valid_member_name(name), "{name:?}");
    }
    for name in ["typecheck", "test:browser", "check_all", "lint-fix"] {
        assert!(is_valid_script_name(name), "{name}");
    }
    for name in ["", "1test", "a b", "a;b", "a$b", "-x"] {
        assert!(!is_valid_script_name(name), "{name:?}");
    }
}

#[test]
fn member_timeout_is_bounded_and_omitted_at_default() {
    let member = |minutes: Value| {
        problem(json!({ "roots": ["."], "members": { "ui": { "timeout_minutes": minutes } } }))
    };
    assert!(member(json!(0)).contains("bad_timeout:0:expected_1..=360"));
    assert!(member(json!(361)).contains("bad_timeout:361:expected_1..=360"));
    assert!(member(json!(0)).contains("stacks.bun.members.timeout_minutes"));
    for minutes in [1, 360] {
        let wire = json!({ "roots": ["."], "members": { "ui": { "timeout_minutes": minutes } } });
        let config = parse(wire.clone());
        config.validate("config.toml").expect("valid");
        assert_eq!(serde_json::to_value(&config).expect("serialize"), wire);
    }
    let wire = json!({ "roots": ["."], "members": { "ui": { "browsers": ["chromium"] } } });
    let config = parse(wire.clone());
    assert_eq!(
        config.members.get("ui").and_then(|m| m.timeout_minutes),
        None
    );
    assert_eq!(serde_json::to_value(&config).expect("serialize"), wire);
}
