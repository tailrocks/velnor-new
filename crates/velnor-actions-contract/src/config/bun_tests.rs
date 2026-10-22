use super::{BunStackConfig, is_valid_member_name, is_valid_script_name, is_valid_suite_id};
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

#[expect(
    clippy::needless_pass_by_value,
    reason = "json! literals read best passed by value"
)]
fn suites(suites: Value) -> Value {
    json!({ "roots": ["."], "members": { "ui": { "suites": suites } } })
}

#[test]
fn suites_validate_round_trip_and_omit_defaults() {
    let wire = suites(json!({
        "unit": { "scripts": ["test"] },
        "browser": { "scripts": ["test:browser"], "browsers": ["chromium", "webkit"], "timeout_minutes": 25 },
        "smoke": { "scripts": ["smoke"], "required": false }
    }));
    let config = parse(wire.clone());
    config.validate("config.toml").expect("valid");
    let member = config.members.get("ui").expect("member");
    assert!(member.suites.get("unit").expect("unit").required);
    assert!(!member.suites.get("smoke").expect("smoke").required);
    assert_eq!(serde_json::to_value(&config).expect("serialize"), wire);
    let explicit = parse(suites(
        json!({ "unit": { "scripts": ["test"], "required": true } }),
    ));
    assert_eq!(
        serde_json::to_value(&explicit).expect("serialize"),
        suites(json!({ "unit": { "scripts": ["test"] } }))
    );
}

#[test]
fn suite_tables_are_strict() {
    for wire in [
        suites(json!({ "unit": { "scripts": ["test"], "run": "bun test" } })),
        suites(json!({ "unit": { "scripts": ["test"], "env": { "A": "b" } } })),
        suites(json!({ "unit": { "browsers": ["chromium"] } })),
        suites(json!({ "unit": { "scripts": ["test"], "browsers": ["opera"] } })),
    ] {
        assert!(serde_json::from_value::<BunStackConfig>(wire).is_err());
    }
}

#[test]
fn every_suite_code_is_reported() {
    let cases = [
        (
            json!({ "Unit": { "scripts": ["test"] } }),
            "bad_suite_id:Unit",
        ),
        (
            json!({ "1unit": { "scripts": ["test"] } }),
            "bad_suite_id:1unit",
        ),
        (
            json!({ "unit-": { "scripts": ["test"] } }),
            "bad_suite_id:unit-",
        ),
        (
            json!({ "a--b": { "scripts": ["test"] } }),
            "bad_suite_id:a--b",
        ),
        (
            json!({ "report": { "scripts": ["test"] } }),
            "reserved_suite_id:report",
        ),
        (
            json!({ "assets": { "scripts": ["test"] } }),
            "reserved_suite_id:assets",
        ),
        (
            json!({ "unit": { "scripts": [] } }),
            "empty_suite_scripts:unit",
        ),
        (
            json!({ "unit": { "scripts": ["a b"] } }),
            "bad_script_name:a b",
        ),
        (
            json!({ "unit": { "scripts": ["test", "test"] } }),
            "duplicate_script:test",
        ),
        (
            json!({ "a": { "scripts": ["test"] }, "b": { "scripts": ["lint", "test"] } }),
            "suite_script_claimed_twice:test",
        ),
        (
            json!({ "unit": { "scripts": ["test"], "browsers": ["webkit", "chromium"] } }),
            "must_be_sorted_and_unique",
        ),
        (
            json!({ "unit": { "scripts": ["test"], "timeout_minutes": 0 } }),
            "bad_timeout:0:expected_1..=360",
        ),
        (
            json!({ "unit": { "scripts": ["test"], "timeout_minutes": 361 } }),
            "bad_timeout:361:expected_1..=360",
        ),
    ];
    for (table, code) in cases {
        let got = problem(suites(table));
        assert!(got.contains(code), "{code}: {got}");
    }
}

#[test]
fn suite_id_grammar() {
    for id in ["unit", "browser", "browser-dev", "e2e2", &"a".repeat(32)] {
        assert!(is_valid_suite_id(id), "{id}");
    }
    for id in [
        "",
        "-a",
        "a-",
        "a--b",
        "A",
        "a_b",
        "a.b",
        "1a",
        &"a".repeat(33),
    ] {
        assert!(!is_valid_suite_id(id), "{id:?}");
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "json! literals read best passed by value"
)]
fn playwright_suite(playwright: Value) -> Value {
    suites(json!({
        "browser": { "scripts": ["test:browser"], "browsers": ["chromium", "webkit"], "playwright": playwright }
    }))
}

#[test]
fn playwright_tables_validate_round_trip_and_omit_defaults() {
    let wire = playwright_suite(json!({
        "projects": ["chromium-desktop", "webkit-desktop"],
        "shards": 4,
        "max_parallel": 8,
        "fail_on_flaky": true
    }));
    let config = parse(wire.clone());
    config.validate("config.toml").expect("valid");
    assert_eq!(serde_json::to_value(&config).expect("serialize"), wire);
    let bare = playwright_suite(json!({}));
    let config = parse(bare.clone());
    config.validate("config.toml").expect("valid");
    let playwright = config.members["ui"].suites["browser"]
        .playwright
        .clone()
        .expect("playwright");
    assert_eq!(playwright.shards, 1);
    assert_eq!(playwright.legs(), 1);
    assert_eq!(serde_json::to_value(&config).expect("serialize"), bare);
    let explicit = parse(playwright_suite(
        json!({ "shards": 1, "fail_on_flaky": false }),
    ));
    assert_eq!(serde_json::to_value(&explicit).expect("serialize"), bare);
}

#[test]
fn playwright_tables_are_strict() {
    for table in [
        json!({ "argv": ["--grep", "x"] }),
        json!({ "reporter": "html" }),
        json!({ "shards": "4" }),
    ] {
        assert!(serde_json::from_value::<BunStackConfig>(playwright_suite(table)).is_err());
    }
}

#[test]
fn every_playwright_code_is_reported() {
    let two_scripts = suites(json!({
        "browser": { "scripts": ["a", "b"], "browsers": ["chromium"], "playwright": {} }
    }));
    assert!(problem(two_scripts).contains("playwright_suite_one_script:browser"));
    let no_browsers = suites(json!({ "browser": { "scripts": ["a"], "playwright": {} } }));
    assert!(problem(no_browsers).contains("playwright_suite_without_browsers:browser"));
    let cases = [
        (json!({ "shards": 0 }), "bad_shards:0:expected_1..=64"),
        (json!({ "shards": 65 }), "bad_shards:65:expected_1..=64"),
        (json!({ "projects": ["a b"] }), "bad_project_name:a b"),
        (json!({ "projects": ["'x'"] }), "bad_project_name:'x'"),
        (json!({ "projects": ["${{x}}"] }), "bad_project_name:${{x}}"),
        (json!({ "projects": ["-x"] }), "bad_project_name:-x"),
        (
            json!({ "projects": ["b", "a"] }),
            "projects_must_be_sorted_and_unique",
        ),
        (
            json!({ "projects": ["a", "a"] }),
            "projects_must_be_sorted_and_unique",
        ),
        (
            json!({ "projects": ["a", "b"], "shards": 33 }),
            "too_many_legs:66:expected_..=64",
        ),
        (json!({ "max_parallel": 0 }), "bad_max_parallel"),
        (
            json!({ "shards": 2, "max_parallel": 3 }),
            "bad_max_parallel",
        ),
    ];
    for (table, code) in cases {
        let got = problem(playwright_suite(table));
        assert!(got.contains(code), "{code}: {got}");
    }
}

#[test]
fn leg_count_is_bounded_at_sixty_four() {
    let at = playwright_suite(json!({ "projects": ["a", "b"], "shards": 32, "max_parallel": 64 }));
    parse(at).validate("config.toml").expect("64 legs");
    let over = playwright_suite(json!({ "projects": ["a", "b", "c", "d", "e"], "shards": 13 }));
    assert!(problem(over).contains("too_many_legs:65:expected_..=64"));
}

#[test]
fn project_name_grammar() {
    for name in ["chromium", "webkit-desktop", "Mobile.Safari", "a:b_c", "1x"] {
        assert!(super::is_valid_project_name(name), "{name}");
    }
    for name in [
        "",
        "-x",
        "a b",
        "a\"b",
        "a'b",
        "a$b",
        "a/b",
        &"a".repeat(65),
    ] {
        assert!(!super::is_valid_project_name(name), "{name:?}");
    }
}

#[test]
fn report_and_retention_are_bounded_and_omitted_at_default() {
    let wire = playwright_suite(json!({
        "shards": 2,
        "report": "merge",
        "report_retention_days": 30,
        "results_retention_days": 1
    }));
    let config = parse(wire.clone());
    config.validate("config.toml").expect("valid");
    assert_eq!(serde_json::to_value(&config).expect("serialize"), wire);
    let defaults = parse(playwright_suite(json!({
        "shards": 2,
        "report": "none",
        "report_retention_days": 14,
        "results_retention_days": 7
    })));
    assert_eq!(
        serde_json::to_value(&defaults).expect("serialize"),
        playwright_suite(json!({ "shards": 2 }))
    );
    let cases = [
        (json!({ "report": "merge" }), "report_requires_shards_or_projects:browser"),
        (json!({ "shards": 2, "report_retention_days": 0 }), "bad_retention_days:0:expected_1..=90"),
        (json!({ "shards": 2, "results_retention_days": 91 }), "bad_retention_days:91:expected_1..=90"),
    ];
    for (table, code) in cases {
        let got = problem(playwright_suite(table));
        assert!(got.contains(code), "{code}: {got}");
    }
    assert!(
        serde_json::from_value::<BunStackConfig>(playwright_suite(json!({ "report": "html" })))
            .is_err()
    );
    parse(playwright_suite(json!({ "projects": ["a", "b"], "report": "merge" })))
        .validate("config.toml")
        .expect("two legs from projects");
}
