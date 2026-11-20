// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The archive split (#600, #601) runs the lint job and the test-archive job in parallel, each with
//! its own target cache. A proc-macro crate and the crates it needs on the host build are compiled
//! identically by `cargo clippy` and by `cargo nextest archive`, so on a cold run both jobs compile
//! them. That is the shape of the split, not a cache miss. A repeat inside one job, or between two
//! jobs that run the same kind of command, is still a miss.

use serde_json::{Value, json};
use velnor_ci::report::{LaneMetrics, check};

const LINT: &[&str] = &[
    "cargo-clippy",
    "clippy",
    "--workspace",
    "--all-targets",
    "--locked",
    "--message-format=json-render-diagnostics",
];
const ARCHIVE: &[&str] = &[
    "cargo-nextest",
    "nextest",
    "archive",
    "--workspace",
    "--locked",
    "--archive-file",
    "nextest-archive.tar.zst",
];

fn artifact(name: &str, kind: &str, extension: &str) -> Value {
    json!({
        "package_id": format!("registry+https://example.invalid/index#{name}@1.0.0"),
        "manifest_path": format!("registry/{name}/Cargo.toml"),
        "target": {"name": name, "kind": [kind], "crate_types": [kind], "src_path": format!("registry/{name}/src/lib.rs")},
        "profile": {"opt_level": "0", "debuginfo": 2, "debug_assertions": true, "overflow_checks": true, "test": false},
        "features": ["default"],
        "filenames": [format!("target/debug/deps/lib{name}-0123abcd.{extension}")],
        "fresh": false
    })
}

/// The proc-macro crate and a library only it needs: both are built whole by either command.
fn host_units() -> Vec<Value> {
    vec![
        artifact("serde_derive", "proc-macro", "so"),
        artifact("syn", "lib", "rlib"),
    ]
}

fn lane(name: &str, argvs: &[&[&str]], artifacts: &[Value]) -> LaneMetrics {
    let mut value = super::report::lane();
    value["lane"] = json!(name);
    value["caches"] = json!([]);
    let template = value["invocations"][0].clone();
    value["invocations"] = argvs
        .iter()
        .map(|argv| {
            let mut invocation = template.clone();
            invocation["argv"] = json!(argv);
            invocation["compilations"] = json!([]);
            invocation["artifacts"] = json!(artifacts);
            invocation
        })
        .collect();
    serde_json::from_value(value).unwrap_or_else(|error| unreachable!("{error}"))
}

fn failure(lanes: &[LaneMetrics]) -> String {
    check(lanes)
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

#[test]
fn lint_and_test_archive_jobs_may_each_compile_the_host_units_on_a_cold_run() {
    let lanes = [
        lane("rust-lint-root", &[LINT], &host_units()),
        lane("rust-test-root", &[ARCHIVE], &host_units()),
    ];
    let result = check(&lanes);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn the_same_unit_twice_in_one_job_is_still_a_miss() {
    let lanes = [lane("rust-test-root", &[ARCHIVE, ARCHIVE], &host_units())];
    let message = failure(&lanes);
    assert!(
        message.contains("syn: identical observed compilation unit executed 2 times"),
        "{message}"
    );
    let lanes = [lane("rust-root", &[LINT, ARCHIVE], &host_units())];
    let message = failure(&lanes);
    assert!(
        message.contains("serde_derive: identical observed compilation unit executed 2 times"),
        "{message}"
    );
}

#[test]
fn two_jobs_running_the_same_kind_of_command_are_still_a_miss() {
    for argv in [LINT, ARCHIVE] {
        let lanes = [
            lane("rust-a", &[argv], &host_units()),
            lane("rust-b", &[argv], &host_units()),
        ];
        let message = failure(&lanes);
        assert!(
            message.contains("syn: identical observed compilation unit executed 2 times"),
            "{message}"
        );
    }
}

#[test]
fn the_split_exemption_does_not_hide_a_third_job() {
    let lanes = [
        lane("rust-lint-root", &[LINT], &host_units()),
        lane("rust-test-root", &[ARCHIVE], &host_units()),
        lane("rust-test-other", &[ARCHIVE], &host_units()),
    ];
    let message = failure(&lanes);
    assert!(
        message.contains("syn: identical observed compilation unit executed 2 times"),
        "{message}"
    );
}
