//! Static settings model tests, including the real `ChainArgos` settings shape.

use velnor_actions_gradle::settings::*;

const ETH: &str = r#"
pluginManagement { repositories { maven { url = uri(file("../build/deps-repo")) } gradlePluginPortal() } }
dependencyResolutionManagement {
    versionCatalogs { create("libs") { from("com.chainargos:libs-catalog:${providers.gradleProperty("v").get()}") } }
}
enableFeaturePreview("TYPESAFE_PROJECT_ACCESSORS")
rootProject.name = "eth-processor-java"
include(
    "eth-catalog",
    "eth-domain", // trailing comment
    "eth-flyway",
)
"#;

fn paths(model: &SettingsModel) -> Vec<&str> {
    model.projects.iter().map(|p| p.path.as_str()).collect()
}

#[test]
fn reads_the_chainargos_kotlin_settings_shape() {
    let model = parse(ETH).expect("static");
    assert_eq!(model.root_name.as_deref(), Some("eth-processor-java"));
    assert_eq!(
        paths(&model),
        [":eth-catalog", ":eth-domain", ":eth-flyway"]
    );
    assert_eq!(model.projects[1].dir, "eth-domain");
}

#[test]
fn reads_groovy_command_syntax_nested_paths_and_project_dirs() {
    let model = parse(
        "rootProject.name = 'g'\ninclude 'a', 'b:c'\ninclude(':d')\n\
         project(':d').projectDir = file('libs/d')\nincludeBuild('build-logic')\n",
    )
    .expect("static");
    assert_eq!(paths(&model), [":a", ":b", ":b:c", ":d"]);
    let dirs: Vec<_> = model.projects.iter().map(|p| p.dir.as_str()).collect();
    assert_eq!(dirs, ["a", "b", "b/c", "libs/d"]);
    assert_eq!(model.included_builds, ["build-logic"]);
}

#[test]
fn groovy_command_syntax_continues_after_a_trailing_comma() {
    let model = parse("include 'a',\n    'b'\ninclude 'c'\n").expect("static");
    assert_eq!(paths(&model), [":a", ":b", ":c"]);
}

#[test]
fn computed_projects_fail_with_the_named_finding() {
    for (text, reason) in [
        ("include(\"a-${name}\")", "interpolated_string"),
        ("include(name)", "non_literal_argument"),
        (
            "rootDir.listFiles()!!.forEach { include(it.name) }",
            "construct:listFiles",
        ),
        ("for (n in names) include(n)", "construct:for"),
        ("includeFlat 'x'", "construct:includeFlat"),
        ("apply(from = \"more.gradle\")", "construct:apply"),
        (
            "include(\"a\")\nproject(\":a\").projectDir = file(dirName)",
            "project_dir_not_literal",
        ),
        (
            "project(\":zzz\").projectDir = file(\"z\")",
            "project_dir_for_unincluded_project",
        ),
        ("include(\"../escape\")", "invalid_project_segment"),
        ("include(\"a\"", "unterminated_call"),
    ] {
        let err = parse(text).expect_err(text);
        assert_eq!(err.reason, reason, "{text}");
        assert_eq!(err.code(), "gradle_dynamic_settings");
    }
}

#[test]
fn comments_and_strings_never_count_as_code() {
    let model =
        parse("// for each x include(\"zzz\")\n/* apply */ include(\"a\")\nval s = \"for\"\n")
            .expect("static");
    assert_eq!(paths(&model), [":a"]);
}
