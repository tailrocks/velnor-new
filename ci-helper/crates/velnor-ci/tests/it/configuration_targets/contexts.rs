// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, fixture, metadata, run};
use serde_json::{Value, json};

fn edge(destination: &str, kind: &Value) -> Value {
    json!({"pkg": destination, "dep_kinds": [kind]})
}

fn ordinary(destination: &str) -> Value {
    edge(destination, &json!({"kind": null, "target": null}))
}

fn report(repo: &Repo, value: &Value) -> String {
    for name in ["first", "alias", "third"] {
        repo.write_unstaged(&format!("{name}.json"), &value.to_string());
    }
    let output = run(repo);
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap_or_else(|e| unreachable!("{e}"))
}

fn assert_target(stdout: &str, package: &str, target: &str, state: &str) {
    assert!(
        stdout.lines().any(|line| {
            line.starts_with(&format!(
                "first: test target {package} ({package}-id) {target} "
            )) && line.ends_with(&format!(": {state}"))
        }),
        "{stdout}"
    );
}

#[test]
fn configuration_targets_keeps_proc_macro_consumers_determinate() {
    let repo = fixture("configuration-target-consumer");
    let mut value = metadata(false);
    value["packages"][0]["targets"]
        .as_array_mut()
        .unwrap_or_else(|| unreachable!())
        .extend([
            json!({"name": "missing", "kind": ["test"], "required-features": ["never"]}),
            json!({"name": "build-script-build", "kind": ["custom-build"]}),
        ]);
    value["packages"][2]["targets"][0]["kind"] = json!(["proc-macro"]);
    value["resolve"]["nodes"][0]["deps"] = json!([ordinary("dep-id")]);
    let stdout = report(&repo, &value);
    assert_target(&stdout, "app", "shared", "enabled");
    assert_target(&stdout, "app", "missing", "unavailable");
    assert_target(&stdout, "member", "shared", "unavailable");
    assert!(
        stdout.contains("no configuration enables test target app (app-id) missing"),
        "{stdout}"
    );
    assert!(
        stdout.contains("no configuration enables test target member (member-id) missing"),
        "{stdout}"
    );
    assert!(stdout.contains("3 configurations, 3 checks"), "{stdout}");
}

#[test]
fn configuration_targets_scopes_hazardous_edges_to_transitive_dependencies() {
    let repo = fixture("configuration-target-edges");
    for hazard in [
        json!({"kind": "build", "target": null}),
        json!({"kind": null, "target": "cfg(windows)"}),
        json!({"kind": "future-kind", "target": null}),
        json!({"kind": null}),
        Value::Null,
    ] {
        let mut value = metadata(false);
        value["workspace_members"] = json!(["app-id", "member-id", "dep-id"]);
        value["packages"][2]["name"] = json!("dep");
        value["resolve"]["nodes"][0]["deps"] = json!([edge("member-id", &hazard)]);
        value["resolve"]["nodes"][1]["deps"] = json!([ordinary("dep-id")]);
        value["resolve"]["nodes"][2]["deps"] = json!([ordinary("member-id")]);
        let stdout = report(&repo, &value);
        assert_target(&stdout, "app", "shared", "enabled");
        assert_target(&stdout, "member", "shared", "unknown (feature contexts)");
        assert_target(
            &stdout,
            "dep",
            "dependency-test",
            "unknown (feature contexts)",
        );
        assert!(stdout.contains("3 configurations, 3 checks"), "{stdout}");
    }
}

#[test]
fn configuration_targets_propagates_package_contexts_through_host_dependencies() {
    let repo = fixture("configuration-target-host");
    for kind in ["proc-macro", "future-target"] {
        let mut value = metadata(false);
        value["packages"][2]["targets"][0]["kind"] = json!([kind]);
        value["packages"].as_array_mut().unwrap_or_else(|| unreachable!()).push(
            json!({"id": "middle-id", "name": "middle", "targets": [{"name": "middle", "kind": ["lib"]}]})
        );
        value["resolve"]["nodes"][0]["deps"] = json!([ordinary("dep-id")]);
        value["resolve"]["nodes"][2]["deps"] = json!([ordinary("middle-id")]);
        value["resolve"]["nodes"]
            .as_array_mut()
            .unwrap_or_else(|| unreachable!())
            .push(json!({"id": "middle-id", "features": [], "deps": [ordinary("member-id")]}));
        let stdout = report(&repo, &value);
        assert_target(&stdout, "app", "shared", "enabled");
        assert_target(&stdout, "member", "shared", "unknown (feature contexts)");
    }
}

#[test]
fn configuration_targets_uses_global_uncertainty_for_unbounded_graphs() {
    let repo = fixture("configuration-target-unbounded");
    let mut values = Vec::new();
    for dependency in [
        json!({"dep_kinds": [{"kind": null, "target": null}]}),
        ordinary("missing-id"),
    ] {
        let mut value = metadata(false);
        value["resolve"]["nodes"][0]["deps"] = json!([dependency]);
        values.push(value);
    }
    let mut missing_deps = metadata(false);
    missing_deps["resolve"]["nodes"][2]
        .as_object_mut()
        .unwrap_or_else(|| unreachable!())
        .remove("deps");
    values.push(missing_deps);
    let mut missing_node = metadata(false);
    missing_node["resolve"]["nodes"][0]["deps"] = json!([ordinary("dep-id")]);
    missing_node["resolve"]["nodes"]
        .as_array_mut()
        .unwrap_or_else(|| unreachable!())
        .pop();
    values.push(missing_node);
    for value in values {
        let stdout = report(&repo, &value);
        assert_target(&stdout, "app", "shared", "unknown (feature contexts)");
        assert_target(&stdout, "member", "shared", "unknown (feature contexts)");
        assert!(!stdout.contains("no configuration enables"), "{stdout}");
    }
}
