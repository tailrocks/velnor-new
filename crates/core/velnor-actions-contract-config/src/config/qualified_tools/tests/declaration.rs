use super::super::*;
use super::bun;

#[test]
fn declared_tool_requires_exact_source_tree_executable_and_probe_identity() {
    assert!(bun().validate("config", "qualified_tools[0]").is_ok());
    for version in ["latest", "v1.3.14", "1.3", "1.3.014", "1.3.14;id"] {
        let mut tool = bun();
        tool.version = version.to_owned();
        assert!(tool.validate("config", "qualified_tools[0]").is_err());
    }
    let mut tool = bun();
    tool.platforms[0].install_tree_sha256 = "0".repeat(64);
    assert!(tool.validate("config", "qualified_tools[0]").is_err());
    let mut tool = bun();
    tool.platforms[0].executables[0].path = "../outside/bun".to_owned();
    assert!(tool.validate("config", "qualified_tools[0]").is_err());
    let mut tool = bun();
    tool.platforms[0].executables[0].probe = QualifiedToolProbe::Version {
        expected: "bun 1.3.140".to_owned(),
    };
    assert!(tool.validate("config", "qualified_tools[0]").is_err());
}

#[test]
fn source_urls_cannot_escape_explicit_backend_provenance() {
    for url in [
        "http://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip",
        "https://attacker.invalid/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip",
        "https://github.com/attacker/bun/releases/download/bun-v1.3.14/bun.zip",
        "https://github.com/oven-sh/bun/releases/download/latest/bun.zip",
        "https://user@github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip",
        "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip?token=x",
        "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip#fragment",
        "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/../bun.zip",
    ] {
        let mut tool = bun();
        tool.platforms[0].artifacts[0].url = url.to_owned();
        assert!(
            tool.validate("config", "qualified_tools[0]").is_err(),
            "{url}"
        );
    }
}

#[test]
fn registry_proves_sorted_unique_dependency_graph() {
    assert!(validate_qualified_tools(&[bun()], "config").is_ok());
    assert!(validate_qualified_tools(&[bun(), bun()], "config").is_err());
    let mut unknown = bun();
    unknown.depends_on = vec!["missing".to_owned()];
    assert!(validate_qualified_tools(&[unknown], "config").is_err());
    let mut first = bun();
    first.id = "a".to_owned();
    first.depends_on = vec!["b".to_owned()];
    let mut second = bun();
    second.id = "b".to_owned();
    second.depends_on = vec!["a".to_owned()];
    assert!(validate_qualified_tools(&[first, second], "config").is_err());
    let mut duplicate_platform = bun();
    duplicate_platform
        .platforms
        .push(duplicate_platform.platforms[0].clone());
    assert!(
        duplicate_platform
            .validate("config", "qualified_tools[0]")
            .is_err()
    );
}

#[test]
fn raw_install_arguments_and_unknown_backends_are_not_schema_fields() {
    for key in ["env", "argv", "plugin_url", "install_args", "settings"] {
        let mut value = serde_json::to_value(bun()).expect("serialized declaration");
        value
            .as_object_mut()
            .expect("object")
            .insert(key.to_owned(), serde_json::json!("evil"));
        assert!(
            serde_json::from_value::<QualifiedTool>(value).is_err(),
            "{key}"
        );
    }
    let mut value = serde_json::to_value(bun()).expect("serialized declaration");
    value["backend"] = serde_json::json!({"kind":"github","repository":"oven-sh/bun"});
    assert!(serde_json::from_value::<QualifiedTool>(value).is_err());
}
