use super::{project_path, validate};

const SETTINGS: &str = r#"
pluginManagement {
    repositories {
        mavenLocal()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositories {
        mavenLocal()
        mavenCentral()
        maven { url = uri("https://packages.example/maven") }
    }
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
}
plugins {
    id("org.gradle.toolchains.foojay-resolver-convention").version("1.0.0")
}
enableFeaturePreview("TYPESAFE_PROJECT_ACCESSORS")
rootProject.name = "generic-monorepo"
include(
    "domain:module",
    "plain-module",
)
"#;

#[test]
fn accepts_reviewed_settings_shape_and_normalizes_selectors() {
    assert!(validate(SETTINGS, None).is_ok());
    assert!(validate("rootProject.name = \"root-only\"", None).is_ok());
    assert!(validate(SETTINGS, Some(":domain:module")).is_ok());
    assert!(validate(SETTINGS, Some("domain:module")).is_ok());
    assert_eq!(
        project_path(":domain:module").as_deref(),
        Ok("domain/module")
    );
    assert_eq!(project_path("plain-module").as_deref(), Ok("plain-module"));
}

#[test]
fn comments_and_strings_cannot_forge_literal_includes() {
    let source = r#"
plugins { id("org.example.settings") }
/* include(":commented-block") project.projectDir = file("else") */
rootProject.name = "generic"
// include(":commented-line")
include(":real")
"#;
    assert_eq!(
        validate(source, Some(":commented-block")),
        Err("gradle_project_not_literal_include")
    );
    assert!(validate(source, Some(":real")).is_ok());
}

#[test]
fn rejects_dynamic_mapping_composites_nested_includes_and_unknown_calls() {
    let cases = [
        (
            "rootProject.name = \"generic\"\ninclude(\":a\")\nunknownCall()",
            "gradle_settings_top_level_unsupported",
        ),
        (
            "rootProject.name = \"generic\"\ninclude(projectName)",
            "gradle_settings_include_dynamic",
        ),
        (
            "plugins { project.projectDir = file(\"else\") }",
            "gradle_settings_project_mapping_unsupported",
        ),
        (
            "pluginManagement { includeBuild(\"tools\") }",
            "gradle_settings_composite_build_unsupported",
        ),
        (
            "pluginManagement { include(\":nested\") }",
            "gradle_settings_include_not_top_level",
        ),
        (
            "if (true) include(\":conditional\")",
            "gradle_settings_top_level_unsupported",
        ),
    ];
    for (source, error) in cases {
        assert_eq!(validate(source, None), Err(error), "{source}");
    }
}

#[test]
fn rejects_interpolation_escaping_backticks_and_unbounded_source() {
    for source in [
        "include(\"${module}\")",
        "include(\"a\\:b\")",
        "include(`module`)",
        "plugins { id(\"x\")",
        "/* unterminated",
    ] {
        assert!(validate(source, None).is_err(), "{source}");
    }
    let oversized = ";".repeat(65_537);
    assert_eq!(
        validate(&oversized, None),
        Err("gradle_settings_too_many_tokens")
    );
}

#[test]
fn rejects_invalid_project_paths() {
    for selector in [":", "::module", ":a::b", ":..", ":a/../b", ":-module"] {
        assert!(project_path(selector).is_err(), "{selector}");
    }
}
