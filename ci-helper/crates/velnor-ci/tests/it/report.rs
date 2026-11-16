// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::report::{LaneMetrics, check, configuration, waste};

pub(crate) fn lane() -> Value {
    json!({"schema":1,"lane":"rust","caches":[{"family":"target","root":".","restore_step":"target_root","save_step":"save_target_root","requested_key":"target-lock-new","matched_key":"target-lock-old","hit":"fallback","bytes":null,"usable":true,"usable_same_lock":true,"save_failed":false}],"invocations":[{"argv":["cargo","check"],"root":".","configuration":"default","started_at":10,"seconds":1.0,"log_start":0,"log_end":0,"compilations":[{"package":"serde","version":"1.0.0","workspace":false,"unchanged":false,"dependencies":[]}],"downloads":[]}],"downloaded":0,"first_cargo_seconds":null,"slowest_tests":[],"tests_run":null,"diagnostics":[],"violations":[]})
}
fn metrics(value: Value) -> LaneMetrics {
    serde_json::from_value(value).unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn cache_waste_rules_respect_restored_provenance_and_root() {
    for (case, expected) in [
        ("dependency", true),
        ("cold", false),
        ("other-lock", false),
        ("workspace", true),
        ("changed", false),
        ("dependency-compiled", false),
        ("download", true),
        ("other-root-download", false),
        ("save-failed", true),
    ] {
        let mut value = lane();
        match case {
            "cold" => {
                value["caches"][0]["usable"] = json!(false);
                value["caches"][0]["usable_same_lock"] = json!(false);
            }
            "other-lock" => value["caches"][0]["usable_same_lock"] = json!(false),
            "workspace" | "changed" | "dependency-compiled" => {
                value["invocations"][0]["compilations"][0]["workspace"] = json!(true);
                value["invocations"][0]["compilations"][0]["unchanged"] = json!(case != "changed");
                if case == "dependency-compiled" {
                    value["invocations"][0]["compilations"][0]["dependencies"] = json!(["serde"]);
                }
            }
            "download" | "other-root-download" => {
                value["caches"][0]["family"] = json!("sources");
                value["caches"][0]["hit"] = json!("exact");
                value["invocations"][0]["downloads"] = json!(["serde v1.0.0"]);
                if case == "other-root-download" {
                    value["caches"][0]["root"] = json!("tools");
                }
            }
            "save-failed" => {
                value["caches"][0]["usable_same_lock"] = json!(false);
                value["caches"][0]["save_failed"] = json!(true);
            }
            _ => {}
        }
        assert_eq!(!waste(&metrics(value)).is_empty(), expected, "{case}");
    }
}

#[test]
fn third_compile_is_aggregated_across_lanes_by_root_configuration_and_package_version() {
    for (dimension, fails) in [
        ("same", true),
        ("plain-modern", true),
        ("package-selection", true),
        ("root", false),
        ("configuration", false),
        ("version", false),
    ] {
        let mut value = lane();
        value["caches"] = json!([]);
        if dimension == "package-selection" {
            value["invocations"][0]["configuration"] = json!(configuration(
                &["cargo", "check", "-p", "a"].map(str::to_owned)
            ));
        }
        if dimension == "plain-modern" {
            value["invocations"][0]["progress"] = json!([]);
        }
        let first = metrics(value.clone());
        value["lane"] = json!("second");
        if dimension == "package-selection" {
            value["invocations"][0]["configuration"] = json!(configuration(
                &["cargo", "check", "--package", "b"].map(str::to_owned)
            ));
        }
        let second = metrics(value.clone());
        assert!(check(&[first.clone(), second.clone()]).is_ok());
        value["lane"] = json!("third");
        match dimension {
            "package-selection" => {
                value["invocations"][0]["configuration"] = json!(configuration(
                    &["cargo", "check", "-p", "c"].map(str::to_owned)
                ));
            }
            "root" => value["invocations"][0]["root"] = json!("tools"),
            "configuration" => value["invocations"][0]["configuration"] = json!("release"),
            "version" => value["invocations"][0]["compilations"][0]["version"] = json!("2.0.0"),
            _ => {}
        }
        assert_eq!(
            check(&[first, second, metrics(value)]).is_err(),
            fails,
            "{dimension}"
        );
    }
    let mut value = lane();
    value["schema"] = json!(2);
    assert!(check(&[metrics(value)]).is_err());
    let mut value = lane();
    value["caches"] = json!([]);
    let value = metrics(value);
    assert!(check(&[value.clone(), value]).is_err());
}

#[test]
fn configuration_ignores_command_purpose_and_normalizes_features() {
    let argv = |words: &[&str]| words.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
        configuration(&argv(&["cargo", "clippy", "--features", "b,a"])),
        configuration(&argv(&[
            "cargo-nextest",
            "nextest",
            "run",
            "--features",
            "a,b"
        ]))
    );
    for words in [
        vec!["cargo", "check", "--features=a,b"],
        vec!["cargo", "check", "--features", "b", "--features", "a"],
    ] {
        assert_eq!(
            configuration(&argv(&words)),
            configuration(&argv(&["cargo", "check", "--features", "b,a"]))
        );
    }
    assert_eq!(
        configuration(&argv(&["cargo", "check", "--release"])),
        configuration(&argv(&["cargo", "check", "--profile=release"]))
    );
    assert_ne!(
        configuration(&argv(&["cargo", "check", "--release"])),
        configuration(&argv(&["cargo", "check"]))
    );
    assert_ne!(
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--target",
            "aarch64-unknown-linux-gnu"
        ])),
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "aarch64-unknown-linux-gnu"
        ]))
    );
    assert_eq!(
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--target",
            "aarch64-unknown-linux-gnu"
        ])),
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "aarch64-unknown-linux-gnu",
            "--target",
            "x86_64-unknown-linux-gnu"
        ]))
    );
    assert_eq!(
        configuration(&argv(&[
            "cargo",
            "check",
            "--target=x86_64-unknown-linux-gnu"
        ])),
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "x86_64-unknown-linux-gnu"
        ])),
    );
}

#[test]
fn observed_units_distinguish_effective_features_outputs_and_repeated_execution() {
    for (case, fails) in [
        ("identical", true),
        ("package-selection", true),
        ("features", false),
        ("metadata-full", false),
        ("test-target", false),
        ("fresh", false),
        ("opaque-unknown", false),
        ("execution", false),
    ] {
        let mut value = lane();
        value["caches"] = json!([]);
        value["invocations"][0]["progress"] = json!([]);
        value["invocations"][0]["artifacts"] = json!([{
            "package_id":"registry+https://example.invalid/index#serde@1.0.0",
            "manifest_path":"registry/serde/Cargo.toml",
            "target":{"name":"serde","kind":["lib"],"crate_types":["lib"],"src_path":"registry/serde/src/lib.rs"},
            "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
            "features":["default"],"filenames":["target/debug/deps/libserde-opaque.rmeta"],"fresh":false
        }]);
        let first = metrics(value.clone());
        value["lane"] = json!("second");
        match case {
            "features" => {
                value["invocations"][0]["artifacts"][0]["features"] = json!(["default", "derive"]);
                value["invocations"][0]["artifacts"][0]["filenames"] =
                    json!(["target/debug/deps/libserde-other.rmeta"]);
            }
            "metadata-full" => {
                value["invocations"][0]["artifacts"][0]["filenames"] =
                    json!(["target/debug/deps/libserde-opaque.rlib"]);
            }
            "test-target" => {
                value["invocations"][0]["artifacts"][0]["profile"]["test"] = json!(true);
            }
            "fresh" => value["invocations"][0]["artifacts"][0]["fresh"] = json!(true),
            "opaque-unknown" => {
                value["invocations"][0]["artifacts"][0]["filenames"] =
                    json!(["target/debug/libserde.dylib"]);
            }
            "package-selection" => {
                value["invocations"][0]["argv"] = json!(["cargo", "check", "-p", "different"]);
            }
            "execution" => {
                value["invocations"][0]["execution"] = json!({"RUSTFLAGS":"-C target-cpu=native"});
            }
            _ => {}
        }
        let second = metrics(value.clone());
        let result = check(&[first, second.clone()]);
        assert_eq!(result.is_err(), fails, "{case}: {result:?}");
        if fails {
            assert!(
                result
                    .err()
                    .is_some_and(|error| error.to_string().contains("serde"))
            );
        }
        if !fails {
            value["lane"] = json!("third");
            value["invocations"][0]["artifacts"][0]["fresh"] = json!(true);
            assert!(check(&[second, metrics(value)]).is_ok(), "{case}");
        }
    }
}
