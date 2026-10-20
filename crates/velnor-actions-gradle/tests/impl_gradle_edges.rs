use std::collections::BTreeSet;

use velnor_actions_gradle::edges::*;

fn known() -> BTreeSet<String> {
    [":eth-domain", ":eth-flyway", ":a", ":a:b", ":snake_case"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[test]
fn accessors_follow_gradle_camel_casing() {
    assert_eq!(accessor(":eth-domain"), "ethDomain");
    assert_eq!(accessor(":a:b"), "a.b");
    assert_eq!(accessor(":snake_case"), "snakeCase");
}

#[test]
fn reads_both_literal_spellings_and_ignores_unknown_and_commented() {
    let script = r#"
        dependencies {
            api(projects.ethFlyway)
            implementation(project(":eth-domain"))
            testImplementation(projects.a.b.something)
            api(projects.unknownThing)
            // implementation(projects.a)
            api(project(":missing"))
            api(project(path = ":a"))
        }
    "#;
    let edges = project_edges(script, &known());
    let edges: Vec<_> = edges.iter().map(String::as_str).collect();
    assert_eq!(edges, [":a:b", ":eth-domain", ":eth-flyway"]);
}
