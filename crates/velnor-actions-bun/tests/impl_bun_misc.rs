use velnor_actions_bun::paths::{is_tsconfig, is_under, normalize, under_root};
use velnor_actions_bun::{check_package_manager, parse_package_json};

// paths
#[test]
fn normalization_folds_dots_and_rejects_escape() {
    assert_eq!(normalize("a/b", "../c").as_deref(), Some("a/c"));
    assert_eq!(normalize("a", "./x/./y").as_deref(), Some("a/x/y"));
    assert_eq!(normalize("a", "../../x"), None);
    assert_eq!(normalize("", "x").as_deref(), Some("x"));
}

#[test]
fn tsconfig_names() {
    assert!(is_tsconfig("tsconfig.json") && is_tsconfig("tsconfig.build.json"));
    assert!(
        !is_tsconfig("tsconfig") && !is_tsconfig("my-tsconfig.json") && !is_tsconfig("tsconfig.js")
    );
}

#[test]
fn roots_and_containment() {
    assert_eq!(under_root(".", "pkg"), "pkg");
    assert_eq!(under_root("frontend", "pkg"), "frontend/pkg");
    assert_eq!(under_root("frontend", ""), "frontend");
    assert!(is_under("a/b/c", "a/b") && is_under("a/b", "a/b") && is_under("x", ""));
    assert!(!is_under("a/bc", "a/b"));
}

// package_json
#[test]
fn facts_are_read_from_both_workspace_spellings() {
    let object = parse_package_json(
        r#"{"name":"r","private":true,"workspaces":{"packages":["a","b/*"],"catalog":{}},
            "scripts":{"test":"vitest run"},"packageManager":"bun@1.4.2"}"#,
    )
    .expect("package");
    assert_eq!(object.workspaces, ["a", "b/*"]);
    assert!(object.private);
    assert!(object.scripts.contains("test"));
    assert_eq!(object.package_manager.as_deref(), Some("bun@1.4.2"));
    let array = parse_package_json(r#"{"workspaces":["x"]}"#).expect("package");
    assert_eq!(array.workspaces, ["x"]);
    assert!(!array.private);
}

#[test]
fn publish_registry_and_invalid_inputs() {
    let package = parse_package_json(
        r#"{"name":"@a/b","version":"1.2.3","publishConfig":{"registry":"https://npm.pkg.github.com"}}"#,
    )
    .expect("package");
    assert_eq!(
        package.publish_registry.as_deref(),
        Some("https://npm.pkg.github.com")
    );
    for text in ["", "[]", "{"] {
        assert_eq!(
            parse_package_json(text).expect_err(text).code(),
            "package_json_invalid"
        );
    }
}

// tools
#[test]
fn versions_must_agree_exactly() {
    assert!(check_package_manager(None, "1.4.2").is_ok());
    assert!(check_package_manager(Some("bun@1.4.2"), "1.4.2").is_ok());
    assert!(check_package_manager(Some("bun@1.4.2+sha512.abc"), "1.4.2").is_ok());
    assert_eq!(
        check_package_manager(Some("bun@1.4.1"), "1.4.2")
            .expect_err("skew")
            .code(),
        "conflicting_project_tool_versions:bun"
    );
    assert_eq!(
        check_package_manager(Some("pnpm@9.0.0"), "1.4.2")
            .expect_err("pnpm")
            .code(),
        "unsupported_package_manager:pnpm@9.0.0"
    );
}
