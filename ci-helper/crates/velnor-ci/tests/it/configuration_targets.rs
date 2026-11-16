// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use serde_json::{Value, json};

use crate::io::Repo;

mod contexts;

fn metadata(extra: bool) -> Value {
    json!({
        "workspace_members": ["app-id", "member-id"],
        "packages": [
            {"id": "app-id", "name": "app", "targets": [
                {"name": "shared", "kind": ["test"], "required-features": ["base"]},
                {"name": "ordinary", "kind": ["test"]},
                {"name": "binary", "kind": ["bin"], "required-features": ["never"]}
            ]},
            {"id": "member-id", "name": "member", "targets": [
                {"name": "shared", "kind": ["test"], "required-features": ["base", "extra"]},
                {"name": "missing", "kind": ["test"], "required-features": ["other-only"]}
            ]},
            {"id": "dep-id", "name": "dependency", "targets": [
                {"name": "dependency-test", "kind": ["test"], "required-features": ["never"]}
            ]}
        ],
        "resolve": {"nodes": [
            {"id": "app-id", "features": ["base", "other-only"], "deps": []},
            {"id": "member-id", "features": if extra {vec!["base", "extra"]} else {vec!["base"]}, "deps": []},
            {"id": "dep-id", "features": [], "deps": []}
        ]}
    })
}

fn fixture(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write_unstaged(
        "bin/cargo",
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> calls
operation=$1
shift
feature=first
while [ "$#" -gt 0 ]; do
    case "$1" in --features) shift; feature=$1 ;; esac
    shift
done
if [ "$operation" = metadata ]; then cat "$feature.json"; fi
"#,
    );
    repo.make_executable("bin/cargo");
    for name in ["first", "alias", "third"] {
        repo.write_unstaged(
            &format!("{name}.json"),
            &metadata(name == "third").to_string(),
        );
    }
    repo.write_unstaged(
        "requests.json",
        &json!([
            {"name": "first", "features": ["first"]},
            {"name": "alias", "features": ["alias"]},
            {"name": "third", "features": ["third"]}
        ])
        .to_string(),
    );
    repo
}

fn run(repo: &Repo) -> Output {
    let path = std::env::join_paths(std::iter::once(repo.root().join("bin")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap_or_else(|e| unreachable!("{e}"));
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["check-configurations", "--configurations", "requests.json"])
        .current_dir(repo.root())
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn configuration_targets_reports_each_request_and_uncovered_workspace_tests() {
    let repo = fixture("configuration-targets");
    let output = run(&repo);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in ["first", "alias", "third"] {
        assert!(
            stdout.contains(&format!(
                "{name}: test target app (app-id) shared [test] requires [base]: enabled"
            )),
            "{stdout}"
        );
        let state = if name == "third" {
            "enabled"
        } else {
            "unavailable"
        };
        assert!(stdout.contains(&format!("{name}: test target member (member-id) shared [test] requires [base, extra]: {state}")), "{stdout}");
        assert!(stdout.contains(&format!("{name}: test target member (member-id) missing [test] requires [other-only]: unavailable")), "{stdout}");
    }
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.starts_with("no configuration enables"))
            .collect::<Vec<_>>(),
        vec![
            "no configuration enables test target member (member-id) missing [test] requires [other-only]"
        ],
        "{stdout}"
    );
    for excluded in ["ordinary", "binary", "dependency-test"] {
        assert!(!stdout.contains(excluded), "{stdout}");
    }
    let calls = fs::read_to_string(repo.root().join("calls")).unwrap_or_default();
    assert_eq!(
        calls
            .lines()
            .filter(|line| line.starts_with("metadata "))
            .count(),
        3,
        "{calls}"
    );
    assert_eq!(
        calls
            .lines()
            .filter(|line| line.starts_with("check "))
            .count(),
        2,
        "{calls}"
    );
    assert!(
        !calls
            .lines()
            .any(|line| line.starts_with("check ") && line.ends_with("alias")),
        "{calls}"
    );
}

#[test]
fn configuration_targets_rejects_incomplete_workspace_metadata_before_checks() {
    let repo = fixture("configuration-targets-invalid");
    let mut invalid = Vec::new();
    for field in ["workspace_members", "packages", "resolve"] {
        let mut value = metadata(false);
        value
            .as_object_mut()
            .unwrap_or_else(|| unreachable!())
            .remove(field);
        invalid.push(value);
    }
    let mut unknown = metadata(false);
    unknown["workspace_members"] = json!(["unknown"]);
    invalid.push(unknown);
    let mut unresolved = metadata(false);
    unresolved["resolve"]["nodes"] = json!([{"id": "app-id", "features": [], "deps": []}]);
    invalid.push(unresolved);
    for field in ["name", "targets"] {
        let mut value = metadata(false);
        value["packages"][0]
            .as_object_mut()
            .unwrap_or_else(|| unreachable!())
            .remove(field);
        invalid.push(value);
    }
    let mut target = metadata(false);
    target["packages"][0]["targets"][0] = json!({"name": "unknown"});
    invalid.push(target);
    for value in invalid {
        repo.write_unstaged("alias.json", &value.to_string());
        repo.write_unstaged("calls", "");
        let output = run(&repo);
        assert_eq!(output.status.code(), Some(1), "{output:?}: {value}");
        let calls = fs::read_to_string(repo.root().join("calls")).unwrap_or_default();
        assert!(
            !calls.lines().any(|line| line.starts_with("check ")),
            "{calls}"
        );
    }
}

#[test]
fn configuration_targets_escapes_external_metadata_names() {
    let repo = fixture("configuration-targets-escape");
    let mut value = metadata(false);
    value["packages"][0]["name"] = json!("::warning::package\n");
    value["packages"][0]["targets"][0]["name"] = json!("##[error]target\r");
    value["packages"][0]["targets"][0]["required-features"] = json!(["::error::feature\n"]);
    for name in ["first", "alias", "third"] {
        repo.write_unstaged(&format!("{name}.json"), &value.to_string());
    }
    let output = run(&repo);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for escaped in [
        "%3A%3Awarning%3A%3Apackage%0A",
        "#%23[error]target%0D",
        "%3A%3Aerror%3A%3Afeature%0A",
    ] {
        assert!(stdout.contains(escaped), "{stdout}");
    }
    assert!(!stdout.contains("::warning::"), "{stdout}");
    assert!(!stdout.contains("##[error]"), "{stdout}");
}

#[test]
fn configuration_targets_reports_unknown_when_feature_contexts_are_ambiguous() {
    let repo = fixture("configuration-targets-context");
    for hazard in [
        json!({"kind": "build", "target": null}),
        json!({"kind": null, "target": "cfg(windows)"}),
    ] {
        let mut value = metadata(false);
        value["resolve"]["nodes"][0]["deps"] = json!([{"dep_kinds": [hazard]}]);
        repo.write_unstaged("alias.json", &value.to_string());
        repo.write_unstaged("calls", "");
        let output = run(&repo);
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("alias: test target app (app-id) shared [test] requires [base]: unknown (feature contexts)"), "{stdout}");
        assert!(!stdout.contains("no configuration enables"), "{stdout}");
        assert!(stdout.contains("3 configurations, 3 checks"), "{stdout}");
    }
}
