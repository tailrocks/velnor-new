// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::io::Repo;
use crate::lane_guard::{Case, Suite, listing, text};

const APP: &str = "app";
const DATABASE: &str = "package(app) and test(/^db_/)";
const SHELL: &str = "binary(it) and test(~cargo_)";
const LIST_ARGS: &str =
    "nextest list --workspace --locked --ignore-default-filter --message-format json";

fn fixture(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write_unstaged(
        "bin/cargo-nextest",
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> calls
printf '   Compiling fake v0.0.0\n' >&2
if [ -f fail ]; then exit 23; fi
filter=
previous=
for argument in "$@"; do
    if [ "$previous" = -E ]; then filter=$argument; fi
    previous=$argument
done
for file in lists/*.filter; do
    if [ "$(cat "$file")" = "$filter" ]; then
        cat "${file%.filter}.json"
        exit 0
    fi
done
exit 24
"#,
    );
    repo.make_executable("bin/cargo-nextest");
    repo
}

fn list_for(repo: &Repo, name: &str, filter: &str, suites: &[Suite<'_>]) {
    repo.write_unstaged(&format!("lists/{name}.filter"), filter);
    repo.write_unstaged(&format!("lists/{name}.json"), &listing(suites));
}

fn check(repo: &Repo, groups: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.args(["test-groups", "check"]);
    for (name, filter) in groups {
        command.args(["--group", &format!("{name}={filter}")]);
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
fn a_test_that_two_groups_match_fails_and_is_named() {
    let repo = fixture("test-groups-overlap");
    let database: &[Case] = &[("db_open", false, true), ("db_close", false, true)];
    let shell: &[Case] = &[("db_open", false, true), ("cargo_build", false, true)];
    list_for(&repo, "database", DATABASE, &[(APP, "listed", database)]);
    list_for(&repo, "shell", SHELL, &[(APP, "listed", shell)]);
    let output = check(&repo, &[("database", DATABASE), ("shell", SHELL)]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let expected = "velnor-ci test-groups: 1 test matches more than one group
  app db_open matches database and shell
give each test to one group: narrow the filter of one of them in [[test_groups]] in .velnor/rust-ci.toml, then run velnor-gen generate
";
    assert!(text(&output.stderr).ends_with(expected), "{output:?}");
    assert_eq!(text(&output.stdout), "");
}

#[test]
fn groups_with_disjoint_tests_pass() {
    let repo = fixture("test-groups-disjoint");
    let database: &[Case] = &[
        ("db_open", false, true),
        ("db_close", false, true),
        ("cargo_build", false, false),
    ];
    let shell: &[Case] = &[
        ("db_open", false, false),
        ("db_close", false, false),
        ("cargo_build", false, true),
    ];
    list_for(&repo, "database", DATABASE, &[(APP, "listed", database)]);
    list_for(&repo, "shell", SHELL, &[(APP, "listed", shell)]);
    let output = check(&repo, &[("database", DATABASE), ("shell", SHELL)]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        text(&output.stdout),
        "velnor-ci test-groups: 2 groups select 3 tests and no test is in two groups\n"
    );
}

#[test]
fn an_ignored_test_in_two_groups_is_exempt() {
    let repo = fixture("test-groups-ignored");
    let database: &[Case] = &[("db_slow", true, true)];
    let shell: &[Case] = &[("db_slow", true, true), ("cargo_build", false, true)];
    list_for(&repo, "database", DATABASE, &[(APP, "listed", database)]);
    list_for(&repo, "shell", SHELL, &[(APP, "listed", shell)]);
    let output = check(&repo, &[("database", DATABASE), ("shell", SHELL)]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn a_test_in_three_groups_names_all_three_and_each_binary_is_told_apart() {
    let repo = fixture("test-groups-three");
    let all: &[Case] = &[("shared", false, true)];
    list_for(
        &repo,
        "a",
        "package(a)",
        &[("one", "listed", all), ("two", "listed", all)],
    );
    list_for(&repo, "b", "package(b)", &[("one", "listed", all)]);
    list_for(&repo, "c", "package(c)", &[("one", "listed", all)]);
    let output = check(
        &repo,
        &[
            ("a", "package(a)"),
            ("b", "package(b)"),
            ("c", "package(c)"),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let expected = "velnor-ci test-groups: 1 test matches more than one group
  one shared matches a, b and c
";
    assert!(
        text(&output.stderr).contains(expected),
        "{}",
        text(&output.stderr)
    );
    assert!(!text(&output.stderr).contains("two shared matches"));
}

#[test]
fn it_lists_each_group_once_with_its_filter_unchanged() {
    let repo = fixture("test-groups-calls");
    let none: &[Case] = &[];
    list_for(&repo, "database", DATABASE, &[(APP, "listed", none)]);
    list_for(&repo, "shell", SHELL, &[(APP, "listed", none)]);
    let output = check(&repo, &[("database", DATABASE), ("shell", SHELL)]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        calls(&repo),
        format!("{LIST_ARGS} -E {DATABASE}\n{LIST_ARGS} -E {SHELL}\n")
    );
    assert!(text(&output.stderr).contains("Compiling fake"));
}

#[test]
fn a_failing_or_garbled_nextest_list_fails_the_check() {
    let repo = fixture("test-groups-broken-list");
    let none: &[Case] = &[];
    list_for(&repo, "database", DATABASE, &[(APP, "listed", none)]);
    repo.write_unstaged("fail", "");
    let output = check(&repo, &[("database", DATABASE), ("shell", SHELL)]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        text(&output.stderr).lines().last(),
        Some("velnor-ci test-groups: cargo-nextest list failed (exit 23)")
    );
    fs::remove_file(repo.root().join("fail")).unwrap_or_else(|e| unreachable!("{e}"));
    for garbled in ["not json", "{}", "[]"] {
        repo.write_unstaged("lists/database.json", garbled);
        let output = check(&repo, &[("database", DATABASE), ("shell", SHELL)]);
        assert_eq!(output.status.code(), Some(1), "{garbled}: {output:?}");
        assert!(
            text(&output.stderr)
                .contains("velnor-ci test-groups: cargo-nextest list returned invalid JSON: "),
            "{garbled}: {output:?}"
        );
        assert_eq!(text(&output.stdout), "");
    }
}

#[test]
fn a_repeated_group_name_or_a_malformed_group_is_refused_before_listing() {
    let repo = fixture("test-groups-refused");
    let repeated = check(&repo, &[("db", DATABASE), ("db", SHELL)]);
    assert_eq!(repeated.status.code(), Some(1), "{repeated:?}");
    assert!(
        text(&repeated.stderr).ends_with("velnor-ci test-groups: the group db is named twice\n"),
        "{repeated:?}"
    );
    for bad in ["no-equals", "=filter", "name="] {
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["test-groups", "check", "--group", bad])
            .current_dir(repo.root())
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(output.status.code(), Some(2), "{bad}: {output:?}");
    }
    assert_eq!(calls(&repo), "");
}
