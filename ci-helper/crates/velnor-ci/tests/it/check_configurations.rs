// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use serde_json::{Value, json};

use crate::io::Repo;

fn fixture(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write_unstaged(
        "bin/cargo",
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> calls
operation=$1
shift
feature=default
while [ "$#" -gt 0 ]; do
    case "$1" in
        --features) shift; feature=$1 ;;
    esac
    shift
done
if [ "$operation" = metadata ]; then
    if [ -f fail-metadata ]; then exit 23; fi
    cat "$feature.json"
else
    if [ -f fail-check ]; then exit 29; fi
fi
"#,
    );
    repo.make_executable("bin/cargo");
    repo
}

fn metadata(nodes: &Value) -> Value {
    json!({
        "packages": [{"id": "app"}, {"id": "dep"}, {"id": "other"}],
        "resolve": {"nodes": nodes}
    })
}

fn graph(app: &[&str], dep: &[&str]) -> Value {
    metadata(&json!([
        {"id": "app", "features": app},
        {"id": "dep", "features": dep}
    ]))
}

fn write_json(repo: &Repo, name: &str, value: &Value) {
    repo.write_unstaged(name, &value.to_string());
}

fn run(repo: &Repo, requests: Option<&Value>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.arg("check-configurations");
    if let Some(requests) = requests {
        write_json(repo, "requests.json", requests);
        command.args(["--manifest-path", "nested/Cargo.toml", "--configurations"]);
        command.arg(repo.root().join("requests.json"));
    }
    let path = std::env::join_paths(std::iter::once(repo.root().join("bin")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap_or_else(|e| unreachable!("{e}"));
    command
        .current_dir(repo.root())
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn calls(repo: &Repo) -> String {
    fs::read_to_string(repo.root().join("calls")).unwrap_or_default()
}

#[test]
fn check_configurations_folds_only_equal_full_package_feature_maps() {
    let repo = fixture("configurations-fold");
    let graphs = [
        ("first", graph(&["base", "stable"], &["shared"])),
        (
            "alias",
            metadata(&json!([
                {"id": "dep", "features": ["shared"]},
                {"id": "app", "features": ["stable", "base"]}
            ])),
        ),
        ("superset", graph(&["base", "stable", "extra"], &["shared"])),
        ("marker", graph(&["base", "stable", "marker"], &["shared"])),
        (
            "dependency",
            graph(&["base", "stable"], &["shared", "extra"]),
        ),
    ];
    for (name, graph) in graphs {
        write_json(&repo, &format!("{name}.json"), &graph);
    }
    let requests = json!([
        {"name": "first", "features": ["first"], "no_default_features": true},
        {"name": "alias", "features": ["alias"]},
        {"name": "superset", "features": ["superset"]},
        {"name": "marker", "features": ["marker"]},
        {"name": "dependency", "features": ["dependency"]}
    ]);
    let output = run(&repo, Some(&requests));
    assert!(output.status.success(), "{output:?}");
    let recorded = calls(&repo);
    let lines: Vec<_> = recorded.lines().collect();
    assert_eq!(lines.len(), 9, "{recorded}");
    assert!(lines[..5].iter().all(|line| line.starts_with("metadata ")));
    assert!(lines[5..].iter().all(|line| line.starts_with("check ")));
    assert_eq!(
        lines[0],
        "metadata --format-version 1 --locked --offline --manifest-path nested/Cargo.toml --no-default-features --features first"
    );
    assert_eq!(
        lines[5],
        "check --workspace --all-targets --locked --offline --manifest-path nested/Cargo.toml --no-default-features --features first"
    );
    assert!(!recorded.contains("--no-deps"));
    assert!(!lines[5..].iter().any(|line| line.ends_with("alias")));
    let summary = String::from_utf8_lossy(&output.stdout);
    assert!(summary.contains("alias -> first"), "{summary}");
    assert!(summary.contains("5 configurations, 4 checks"), "{summary}");
}

#[test]
fn check_configurations_keeps_package_identity_and_resolves_again_on_every_run() {
    let repo = fixture("configurations-fresh");
    write_json(&repo, "one.json", &graph(&[], &["same"]));
    write_json(&repo, "two.json", &graph(&[], &["same"]));
    let requests = json!([
        {"name": "one", "features": ["one"]},
        {"name": "two", "features": ["two"]}
    ]);
    assert!(run(&repo, Some(&requests)).status.success());
    assert_eq!(
        calls(&repo)
            .lines()
            .filter(|l| l.starts_with("check "))
            .count(),
        1
    );
    write_json(
        &repo,
        "two.json",
        &metadata(&json!([
            {"id": "app", "features": []}, {"id": "other", "features": ["same"]}
        ])),
    );
    repo.write_unstaged("calls", "");
    assert!(run(&repo, Some(&requests)).status.success());
    assert_eq!(
        calls(&repo)
            .lines()
            .filter(|l| l.starts_with("check "))
            .count(),
        2
    );
}

#[test]
fn check_configurations_rejects_invalid_metadata_before_any_check() {
    let repo = fixture("configurations-invalid-metadata");
    write_json(&repo, "valid.json", &graph(&[], &[]));
    let requests = json!([
        {"name": "valid", "features": ["valid"]},
        {"name": "invalid", "features": ["invalid"]}
    ]);
    let invalid = [
        json!({}),
        json!({"packages": [], "resolve": null}),
        metadata(&json!([])),
        metadata(&json!([{"id": "unknown", "features": []}])),
        metadata(&json!([{"id": "app", "features": []}, {"id": "app", "features": []}])),
        json!({"packages": [{"id": "app"}, {"id": "app"}], "resolve": {"nodes": [{"id": "app", "features": []}]}}),
    ];
    for graph in invalid {
        write_json(&repo, "invalid.json", &graph);
        repo.write_unstaged("calls", "");
        let output = run(&repo, Some(&requests));
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(!calls(&repo).lines().any(|line| line.starts_with("check ")));
    }
    repo.write_unstaged("invalid.json", "not JSON");
    assert_eq!(run(&repo, Some(&requests)).status.code(), Some(1));
    assert!(!calls(&repo).lines().any(|line| line.starts_with("check ")));
}

#[test]
fn check_configurations_validates_requests_and_propagates_cargo_failures() {
    let repo = fixture("configurations-failures");
    write_json(&repo, "default.json", &graph(&[], &[]));
    for requests in [
        json!([]),
        json!([{"name": ""}]),
        json!([{"name": "same"}, {"name": "same"}]),
        json!([{"name": "::warning::injected"}]),
        json!([{"name": "safe", "features": ["--all-features"]}]),
        json!([{"name": "safe", "command": "anything"}]),
    ] {
        assert_eq!(run(&repo, Some(&requests)).status.code(), Some(1));
        assert_eq!(calls(&repo), "");
    }
    repo.write_unstaged("fail-metadata", "");
    let output = run(&repo, None);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("23"));
    assert!(!calls(&repo).contains("check "));
    fs::remove_file(repo.root().join("fail-metadata")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("fail-check", "");
    let output = run(&repo, None);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("29"));
}

#[test]
fn check_configurations_without_a_request_file_checks_default_once() {
    let repo = fixture("configurations-default");
    write_json(&repo, "default.json", &graph(&[], &[]));
    let output = run(&repo, None);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        calls(&repo),
        "metadata --format-version 1 --locked --offline --manifest-path Cargo.toml\ncheck --workspace --all-targets --locked --offline --manifest-path Cargo.toml\n"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 configurations, 1 checks"));
}
