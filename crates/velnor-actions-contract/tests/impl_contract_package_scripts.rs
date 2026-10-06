//! Package obligations are explicit closed script selections.

use velnor_actions_contract::config::{PackageScript, WorkloadConfig};

fn parse(kind: &str, scripts: &serde_json::Value) -> Result<WorkloadConfig, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "name": "package", "kind": kind, "scripts": scripts,
    }))
}

#[test]
fn defaults_preserve_bun_but_node_requires_explicit_scripts() -> Result<(), serde_json::Error> {
    let bun = parse("bun_ci", &serde_json::Value::Null)?;
    assert!(bun.validate("config.toml").is_ok());
    assert_eq!(
        bun.package_scripts(),
        [PackageScript::Build, PackageScript::Test]
    );
    let node = parse("node_ci", &serde_json::Value::Null)?;
    assert!(node.validate("config.toml").is_err());
    Ok(())
}

#[test]
fn scripts_are_nonempty_unique_canonical_and_package_only() -> Result<(), serde_json::Error> {
    for scripts in [
        serde_json::json!([]),
        serde_json::json!(["build", "build"]),
        serde_json::json!(["test", "build"]),
    ] {
        for kind in ["bun_ci", "node_ci"] {
            assert!(
                parse(kind, &scripts)
                    .is_ok_and(|workload| workload.validate("config.toml").is_err())
            );
        }
    }
    let scripts = serde_json::json!(["lint", "typecheck", "check", "build", "test"]);
    for kind in ["bun_ci", "node_ci"] {
        assert!(parse(kind, &scripts)?.validate("config.toml").is_ok());
    }
    assert!(
        parse("docker_build", &scripts)?
            .validate("config.toml")
            .is_err()
    );
    Ok(())
}

#[test]
fn arbitrary_script_names_and_arguments_are_rejected_by_deserialization() {
    for script in ["deploy", "build --force", "$(id)", "lint;id", "test:unit"] {
        assert!(
            serde_json::from_value::<WorkloadConfig>(serde_json::json!({
                "name":"package", "kind":"bun_ci", "scripts":[script]
            }))
            .is_err(),
            "{script}"
        );
    }
}
