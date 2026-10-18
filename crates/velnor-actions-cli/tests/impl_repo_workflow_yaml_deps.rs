//! Exact YAML parser dependencies for the workflow renderer.

use std::error::Error;

use crate::impl_repo_policy::{MEMBERS, manifest, read};

const OWNER: &str = "crates/velnor-actions-workflow-renderer";
const PARSERS: [&str; 2] = ["saphyr", "saphyr-parser"];

fn declaration_is_exact(value: &toml::Value) -> bool {
    value.as_table().is_some_and(|fields| {
        fields.len() == 2
            && fields.get("version").and_then(toml::Value::as_str) == Some("=0.2.2")
            && fields
                .get("default-features")
                .and_then(toml::Value::as_bool)
                == Some(false)
    })
}

/// Accept only the renderer's exact parser versions without default features.
pub(crate) fn reviewed_workflow_yaml_dependency(dir: &str, key: &str, line: &str) -> bool {
    if dir != OWNER || !PARSERS.contains(&key) {
        return false;
    }
    let Ok(fields) = toml::from_str::<toml::Table>(line) else {
        return false;
    };
    fields.len() == 1 && fields.get(key).is_some_and(declaration_is_exact)
}

fn parser_name(value: &str) -> bool {
    PARSERS.contains(&value)
}

fn table_is_exact(dir: &str, table: &toml::Table, path: &mut Vec<String>) -> bool {
    for (key, value) in table {
        let alias = value
            .as_table()
            .and_then(|fields| fields.get("package"))
            .and_then(toml::Value::as_str)
            .is_some_and(parser_name);
        let dependency_section = match path.as_slice() {
            [section] | [_, section] if path.len() == 1 || path[0] == "workspace" => {
                matches!(
                    section.as_str(),
                    "dependencies" | "dev-dependencies" | "build-dependencies"
                )
            }
            [target, _, section] if target == "target" => {
                matches!(
                    section.as_str(),
                    "dependencies" | "dev-dependencies" | "build-dependencies"
                )
            }
            _ => false,
        };
        if dependency_section
            && (parser_name(key) || alias)
            && (dir != OWNER
                || path.as_slice() != ["dependencies"]
                || !PARSERS.contains(&key.as_str())
                || !declaration_is_exact(value))
        {
            return false;
        }
        if let Some(children) = value.as_table() {
            path.push(key.clone());
            let valid = table_is_exact(dir, children, path);
            path.pop();
            if !valid {
                return false;
            }
        }
    }
    true
}

fn dependency_sections_are_exact(dir: &str, text: &str) -> bool {
    let Ok(doc) = toml::from_str::<toml::Table>(text) else {
        return false;
    };
    // Source overrides are outside the approved dependency contract.
    if ["patch", "replace"].iter().any(|name| {
        doc.get(*name)
            .and_then(toml::Value::as_table)
            .is_some_and(|overrides| !overrides.is_empty())
    }) {
        return false;
    }
    table_is_exact(dir, &doc, &mut Vec::new())
}

#[test]
fn workflow_yaml_policy_rejects_other_owners_versions_and_options() {
    for parser in PARSERS {
        let line = format!("{parser} = {{ version = \"=0.2.2\", default-features = false }}");
        assert!(reviewed_workflow_yaml_dependency(OWNER, parser, &line));
        for owner in [
            "crates/velnor-actions-cli",
            "crates/velnor-actions-orchestrator",
        ] {
            assert!(!reviewed_workflow_yaml_dependency(owner, parser, &line));
        }
        for altered in [
            line.replace("=0.2.2", "0.2.2"),
            line.replace("=0.2.2", "=0.2.1"),
            line.replace("false", "true"),
            line.replace(", default-features = false", ""),
            line.replace("false", "\"false\""),
            line.replace(" }", ", features = [\"debug_prints\"] }"),
            line.replace(" }", ", optional = true }"),
            line.replace(" }", ", package = \"other\" }"),
            line.replace(" }", ", path = \"../other\" }"),
            line.replace(" }", ", git = \"https://example.invalid/other\" }"),
            line.replace(" }", ", version = \"=0.2.2\" }"),
        ] {
            assert!(!reviewed_workflow_yaml_dependency(OWNER, parser, &altered));
        }
        assert!(!reviewed_workflow_yaml_dependency(OWNER, "other", &line));
    }
}

#[test]
fn workflow_yaml_dependencies_stay_in_the_renderer_normal_dependencies()
-> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        assert!(dependency_sections_are_exact(dir, &manifest(dir)?), "{dir}");
    }
    assert!(dependency_sections_are_exact(".", &read("Cargo.toml")?));
    for section in [
        "dev-dependencies",
        "build-dependencies",
        "target.'cfg(unix)'.dependencies",
        "workspace.dependencies",
        "patch.crates-io",
    ] {
        let text =
            format!("[{section}]\nsaphyr = {{ version = \"=0.2.2\", default-features = false }}\n");
        assert!(!dependency_sections_are_exact(OWNER, &text));
    }
    assert!(dependency_sections_are_exact(
        OWNER,
        r#"[package.metadata]
saphyr = "YAML parser documentation"
"#,
    ));
    let nested = "[dependencies.saphyr]\nversion = \"=0.2.2\"\ndefault-features = false\n";
    assert!(dependency_sections_are_exact(OWNER, nested));
    Ok(())
}

#[test]
fn workflow_yaml_policy_reads_quoted_keys_headers_aliases_and_overrides() {
    for text in [
        r#"[target.'cfg(unix)'.dependencies]
"saphyr" = { version = "=0.2.2", default-features = true }
"#,
        r#"[target.'cfg(unix)'.dependencies."saphyr"]
version = "=0.2.2"
default-features = true
"#,
        r#"["target"."cfg(unix)"."dependencies"]
"saph\u0079r" = { version = "=0.2.2", default-features = false }
"#,
        r#"[target.'cfg(unix)'.dependencies]
serde = { "pack\u0061ge" = "saphyr", version = "=0.2.2", default-features = false }
"#,
        r#"[patch.crates-io]
"saphyr" = { path = "../other" }
"#,
        r#"[replace]
"saphyr:0.2.2" = { path = "../other" }
"#,
        r#"[dependencies]
saphyr = { version = "=0.2.2", "default-features" = "false" }
"#,
        r#"[replace]
"saphyr@0.2.2" = { path = "replacement" }
"#,
        r#"[replace]
"https://github.com/rust-lang/crates.io-index#saphyr@0.2.2" = { path = "replacement" }
"#,
        r#"[replace]
"registry+https://github.com/rust-lang/crates.io-index#saphyr@0.2.2" = { path = "replacement" }
"#,
        r#"[patch.crates-io]
other = { package = "other", path = "replacement" }
"#,
        "not valid TOML",
    ] {
        assert!(!dependency_sections_are_exact(OWNER, text), "{text}");
    }
}
