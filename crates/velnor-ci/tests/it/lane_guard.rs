// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use serde_json::{Map, Value, json};

use crate::io::Repo;

const DEFAULT_LIST: &str = include_str!("data/lane_guard/default.json");
const FEATURES_LIST: &str = include_str!("data/lane_guard/features.json");
const SELECTED_LIST: &str = include_str!("data/lane_guard/selected.json");

const HOST: &str = "velnor-runner-host";
const PAIR: &str = "velnor-runner-host::local_pair_public";
const LIST_ARGS: &str =
    "nextest list --workspace --locked --ignore-default-filter --message-format json";

pub(crate) type Case = (&'static str, bool, bool);
pub(crate) type Suite<'a> = (&'a str, &'a str, &'a [Case]);

fn fixture(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write_unstaged(
        "bin/cargo-nextest",
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> calls
printf '   Compiling fake v0.0.0\n' >&2
if [ -f fail ]; then exit 23; fi
features=0
selected=0
previous=
for argument in "$@"; do
    if [ "$argument" = --features ]; then features=1; fi
    if [ "$previous" = -E ]; then
        case "$argument" in "("*) selected=1 ;; esac
    fi
    previous=$argument
done
if [ "$selected" = 1 ]; then cat s.json
elif [ "$features" = 1 ]; then cat l.json
else cat d.json
fi
"#,
    );
    repo.make_executable("bin/cargo-nextest");
    repo
}

pub(crate) fn listing(suites: &[Suite<'_>]) -> String {
    let mut rust_suites = Map::new();
    for (id, status, cases) in suites {
        let mut testcases = Map::new();
        for (name, ignored, matches) in *cases {
            testcases.insert(
                (*name).to_owned(),
                json!({
                    "kind": "test",
                    "ignored": ignored,
                    "filter-match": {"status": if *matches { "matches" } else { "mismatch" }}
                }),
            );
        }
        rust_suites.insert(
            (*id).to_owned(),
            json!({"binary-id": id, "status": status, "testcases": testcases}),
        );
    }
    json!({"test-count": 0, "rust-suites": Value::Object(rust_suites)}).to_string()
}

fn lists(repo: &Repo, default: &str, features: &str, selected: &str) {
    repo.write_unstaged("d.json", default);
    repo.write_unstaged("l.json", features);
    repo.write_unstaged("s.json", selected);
}

fn guard(repo: &Repo, features: &str, filter: &str, except: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.args(["lane-guard", "--features", features, "--filter", filter]);
    for binary in except {
        command.args(["--except-binary", binary]);
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

pub(crate) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn fails_naming_each_feature_only_test_outside_the_filter() {
    let repo = fixture("lane-guard-fails");
    let shared: &[Case] = &[("tests::shared", false, true)];
    let feature_only: &[Case] = &[
        ("tests::shared", false, true),
        ("tests::other_case", false, false),
        ("tests::new_case", false, false),
    ];
    let mismatch: &[Case] = &[
        ("tests::shared", false, true),
        ("tests::other_case", false, false),
        ("tests::new_case", false, false),
    ];
    lists(
        &repo,
        &listing(&[(PAIR, "listed", shared)]),
        &listing(&[(PAIR, "listed", feature_only)]),
        &listing(&[(PAIR, "listed", mismatch)]),
    );
    let output = guard(
        &repo,
        "local-mode",
        "test(~local::) or binary(local_host_public)",
        &[],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = text(&output.stderr);
    let expected = format!(
        "velnor-ci lane-guard: 2 tests exist only with features 'local-mode' and fall outside the lane filter
  filter: test(~local::) or binary(local_host_public)
  not selected: {PAIR} tests::new_case
  not selected: {PAIR} tests::other_case
extend the filter of the [[test_lane]] in .velnor/rust-ci.toml so it selects them, then run velnor-gen generate
"
    );
    assert!(stderr.ends_with(&expected), "{stderr}");
    assert!(stderr.contains("Compiling fake"), "{stderr}");
    assert_eq!(text(&output.stdout), "");
}

#[test]
fn passes_when_every_feature_only_test_is_selected() {
    let repo = fixture("lane-guard-passes");
    let shared: &[Case] = &[("tests::shared", false, true)];
    let all: &[Case] = &[
        ("tests::shared", false, true),
        ("tests::new_case", false, true),
        ("tests::other_case", false, true),
    ];
    lists(
        &repo,
        &listing(&[(HOST, "listed", shared)]),
        &listing(&[(HOST, "listed", all)]),
        &listing(&[(HOST, "listed", all)]),
    );
    let output = guard(&repo, "local-mode", "test(~tests::)", &[]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        text(&output.stdout),
        "velnor-ci lane-guard: all 2 tests that exist only with features 'local-mode' are selected by the lane filter\n"
    );
}

#[test]
fn shared_tests_outside_the_filter_do_not_fail() {
    let repo = fixture("lane-guard-shared");
    let shared: &[Case] = &[("tests::shared", false, true)];
    let features: &[Case] = &[("tests::shared", false, true), ("tests::only", false, true)];
    let selected: &[Case] = &[
        ("tests::shared", false, false),
        ("tests::only", false, true),
    ];
    lists(
        &repo,
        &listing(&[(HOST, "listed", shared)]),
        &listing(&[(HOST, "listed", features)]),
        &listing(&[(HOST, "listed", selected)]),
    );
    let output = guard(&repo, "local-mode", "test(=tests::only)", &[]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        text(&output.stdout),
        "velnor-ci lane-guard: the 1 test that exists only with features 'local-mode' is selected by the lane filter\n"
    );
}

#[test]
fn a_feature_only_test_in_a_binary_the_filter_skips_fails() {
    let repo = fixture("lane-guard-skipped-binary");
    lists(&repo, DEFAULT_LIST, FEATURES_LIST, SELECTED_LIST);
    let output = guard(&repo, "local", "binary_id(=fx)", &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let expected = "velnor-ci lane-guard: 1 test exists only with features 'local' and falls outside the lane filter
  filter: binary_id(=fx)
  not selected: fx::local_pub pub_case
extend the filter of the [[test_lane]] in .velnor/rust-ci.toml so it selects it, then run velnor-gen generate
";
    assert!(text(&output.stderr).ends_with(expected), "{output:?}");
}

#[test]
fn ignored_feature_only_tests_are_exempt() {
    let repo = fixture("lane-guard-ignored");
    let shared: &[Case] = &[("tests::shared", false, true)];
    let features: &[Case] = &[("tests::shared", false, true), ("tests::slow", true, false)];
    lists(
        &repo,
        &listing(&[(HOST, "listed", shared)]),
        &listing(&[(HOST, "listed", features)]),
        &listing(&[(HOST, "listed", features)]),
    );
    let output = guard(&repo, "local-mode", "test(=tests::shared)", &[]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        text(&output.stdout),
        "velnor-ci lane-guard: no runnable tests exist only with features 'local-mode'; nothing to check\n"
    );
}

#[test]
fn a_failing_or_garbled_nextest_list_fails_the_guard() {
    let repo = fixture("lane-guard-broken-list");
    let empty: &[Case] = &[];
    let valid = listing(&[(HOST, "listed", empty)]);
    lists(&repo, &valid, &valid, &valid);
    repo.write_unstaged("fail", "");
    let output = guard(&repo, "local-mode", "all()", &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        text(&output.stderr).lines().last(),
        Some("velnor-ci lane-guard: cargo-nextest list failed (exit 23)")
    );
    assert_eq!(calls(&repo).lines().count(), 1);
    fs::remove_file(repo.root().join("fail")).unwrap_or_else(|e| unreachable!("{e}"));
    for garbled in ["not json", "{}", "[]"] {
        lists(&repo, garbled, &valid, &valid);
        let output = guard(&repo, "local-mode", "all()", &[]);
        assert_eq!(output.status.code(), Some(1), "{garbled}: {output:?}");
        assert!(
            text(&output.stderr)
                .contains("velnor-ci lane-guard: cargo-nextest list returned invalid JSON: "),
            "{garbled}: {output:?}"
        );
        assert_eq!(text(&output.stdout), "");
    }
}

#[test]
fn it_runs_three_lists_with_the_expected_arguments() {
    let repo = fixture("lane-guard-calls");
    let empty: &[Case] = &[];
    let valid = listing(&[(HOST, "listed", empty)]);
    lists(&repo, &valid, &valid, &valid);
    let output = guard(
        &repo,
        "local-mode,extra",
        "test(~local::)",
        &["velnor-runner-host::direct", "other"],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let expected = format!(
        "{LIST_ARGS} -E all() - binary_id(=velnor-runner-host::direct) - binary_id(=other)
{LIST_ARGS} -E all() - binary_id(=velnor-runner-host::direct) - binary_id(=other) --features local-mode,extra
{LIST_ARGS} --features local-mode,extra -E (test(~local::)) - binary_id(=velnor-runner-host::direct) - binary_id(=other)
"
    );
    assert_eq!(calls(&repo), expected);

    let plain = fixture("lane-guard-calls-plain");
    lists(&plain, &valid, &valid, &valid);
    assert_eq!(guard(&plain, "a", "all()", &[]).status.code(), Some(0));
    let expected = format!(
        "{LIST_ARGS}
{LIST_ARGS} --features a
{LIST_ARGS} --features a -E (all())
"
    );
    assert_eq!(calls(&plain), expected);
}
