// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, fixture, metadata, run};
use serde_json::{Value, json};

fn report(repo: &Repo, value: &Value) -> String {
    for name in ["first", "alias", "third"] {
        let mut value = value.clone();
        value["resolve"]["nodes"][2]["features"] = if name == "third" {
            json!(["marker"])
        } else {
            json!([])
        };
        repo.write_unstaged(&format!("{name}.json"), &value.to_string());
    }
    let output = run(repo);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap_or_else(|e| unreachable!("{e}"));
    for name in ["first", "alias", "third"] {
        assert!(
            stdout.lines().any(|line| {
                line.starts_with(&format!("{name}: test target app (app-id) shared "))
                    && line.ends_with(": unknown (dependency feature requirements)")
            }),
            "{stdout}"
        );
    }
    assert!(
        !stdout.contains("no configuration enables test target app"),
        "{stdout}"
    );
    stdout
}

fn qualified(requirements: &[&str]) -> Value {
    let mut value = metadata(false);
    value["packages"][0]["targets"][0]["required-features"] = json!(requirements);
    value
}

#[test]
fn configuration_targets_reports_dependency_requirements_unknown_with_features_on_and_off() {
    let repo = fixture("qualified-on-off");
    let mut value = qualified(&["dep/marker"]);
    value["resolve"]["nodes"][0]["deps"] = json!([
        {"name": "dep", "pkg": "dep-id", "dep_kinds": [{"kind": null, "target": null}]}
    ]);
    let stdout = report(&repo, &value);
    assert!(stdout.contains("no configuration enables test target member (member-id) missing [test] requires [other-only]"), "{stdout}");
    assert!(stdout.contains("3 configurations, 2 checks"), "{stdout}");
}

#[test]
fn configuration_targets_does_not_guess_dependency_identity_or_optional_activation() {
    let repo = fixture("qualified-identity");
    for edge_name in ["renamed_dep", "custom_library", "absent"] {
        let mut value = qualified(&["manifest_alias/marker"]);
        value["packages"][0]["dependencies"] = json!([
            {"name": "dependency", "rename": "manifest_alias", "optional": true}
        ]);
        value["packages"][2]["targets"][0]["name"] = json!("custom_library");
        let edge = json!({"name": edge_name, "pkg": "dep-id", "dep_kinds": [{"kind": null, "target": null}]});
        if edge_name == "absent" {
            value["resolve"]["nodes"][1]["deps"] = json!([edge]);
        } else {
            value["resolve"]["nodes"][0]["deps"] = json!([edge]);
        }
        report(&repo, &value);
    }
}

#[test]
fn configuration_targets_prioritizes_dependency_requirements_over_contexts_and_missing_local_features()
 {
    let repo = fixture("qualified-contexts");
    for context in ["missing", "hazard", "ordinary"] {
        let mut value = qualified(&["missing-local", "dep/marker"]);
        if context == "missing" {
            value["resolve"]["nodes"][0]
                .as_object_mut()
                .unwrap_or_else(|| unreachable!())
                .remove("deps");
        } else if context == "hazard" {
            value["resolve"]["nodes"][1]["deps"] = json!([
                {"pkg": "app-id", "dep_kinds": [{"kind": "build", "target": null}]}
            ]);
        }
        report(&repo, &value);
    }
}
