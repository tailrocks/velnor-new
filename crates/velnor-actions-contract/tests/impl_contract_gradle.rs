//! Closed Gradle selectors and local database descriptors.
use velnor_actions_contract::WorkloadConfig;

#[test]
fn gradle_goals_require_safe_descriptors() {
    let parse = |value| serde_json::from_value::<WorkloadConfig>(value).expect("shape");
    let base = serde_json::json!({"name":"check","kind":"gradle_check","gradle":{"project":":domain:core"}});
    assert!(parse(base.clone()).validate("config.toml").is_ok());
    for project in [
        "domain:core",
        ":",
        ":a::b",
        ":..",
        ":-a",
        ":a;id",
        ":${{x}}",
    ] {
        let mut value = base.clone();
        value["gradle"]["project"] = project.into();
        assert!(parse(value).validate("config.toml").is_err(), "{project}");
    }
    let mut missing = base.clone();
    missing.as_object_mut().expect("object").remove("gradle");
    assert!(parse(missing).validate("config.toml").is_err());
    let mut wrong = base;
    wrong["kind"] = "reuse".into();
    assert!(parse(wrong).validate("config.toml").is_err());
}

#[test]
fn database_fixture_requires_local_literals_and_declared_databases() {
    let base = serde_json::json!({"name":"check","kind":"gradle_database_check","gradle":{"postgres":{
        "user":"fixture","password":"local-fixture","databases":["fixture"],
        "bindings":{"APP_DB_URL":{"kind":"jdbc_url","database":"fixture"}}
    }}});
    let parse = |value| serde_json::from_value::<WorkloadConfig>(value).expect("shape");
    assert!(parse(base.clone()).validate("config.toml").is_ok());
    for name in [
        "PATH",
        "LD_PRELOAD",
        "GITHUB_ENV",
        "JAVA_TOOL_OPTIONS",
        "GRADLE_DB_URL",
        "GITHUB_DB_URL",
        "APP_DB_HOST",
    ] {
        let mut value = base.clone();
        value["gradle"]["postgres"]["bindings"] =
            serde_json::json!({name:{"kind":"jdbc_url","database":"fixture"}});
        assert!(parse(value).validate("config.toml").is_err(), "{name}");
    }
    for bindings in [
        serde_json::json!({}),
        serde_json::json!({"APP_DB_HOST":{"kind":"host"}}),
        serde_json::json!({"APP_DB_PORT":{"kind":"port"}}),
    ] {
        let mut value = base.clone();
        value["gradle"]["postgres"]["bindings"] = bindings;
        assert!(parse(value).validate("config.toml").is_err());
    }
    for password in ["", "${{secrets.x}}", "$(id)", "a\nb", "a'b"] {
        let mut value = base.clone();
        value["gradle"]["postgres"]["password"] = password.into();
        assert!(parse(value).validate("config.toml").is_err());
    }
    let mut undeclared = base;
    undeclared["gradle"]["postgres"]["bindings"]["APP_DB_URL"]["database"] = "other".into();
    assert!(parse(undeclared).validate("config.toml").is_err());
}
