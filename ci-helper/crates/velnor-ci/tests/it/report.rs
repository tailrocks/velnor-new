// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::report::{LaneMetrics, check, waste};
use velnor_ci::telemetry::configuration;

pub(crate) fn lane() -> Value {
    json!({"schema":1,"lane":"rust","caches":[{"family":"target","root":".","restore_step":"target_root","save_step":"save_target_root","requested_key":"target-lock-new","matched_key":"target-lock-old","hit":"fallback","bytes":null,"usable":true,"usable_same_lock":true,"save":"not_attempted"}],"invocations":[{"argv":["cargo","check"],"root":".","configuration":"default","started_at":10,"seconds":1.0,"log_start":0,"log_end":0,"compilations":[{"package":"serde","version":"1.0.0","workspace":false,"unchanged":false,"dependencies":[]}],"downloads":[],"capture_complete":true}],"downloaded":0,"first_cargo_seconds":null,"slowest_tests":[],"tests_run":null,"diagnostics":[],"violations":[]})
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
                value["caches"][0]["save"] = json!("failed");
            }
            _ => {}
        }
        assert_eq!(!waste(&metrics(value)).is_empty(), expected, "{case}");
    }
}

#[test]
fn only_workspace_dependency_compilation_exempts_unchanged_workspace_crates() {
    for workspace in [false, true] {
        let mut value = lane();
        value["caches"][0]["usable_same_lock"] = json!(false);
        value["invocations"][0]["compilations"] = json!([
            {"package":"app","version":"1.0.0","workspace":true,"unchanged":true,"dependencies":["helper"]},
            {"package":"helper","version":if workspace { "1.0.0" } else { "2.0.0" },"workspace":workspace,"unchanged":false,"dependencies":[]}
        ]);
        let violations = waste(&metrics(value));
        if workspace {
            assert!(violations.is_empty(), "{violations:?}");
        } else {
            assert_eq!(
                violations,
                [".: app v1.0.0 compiled despite usable restored inputs"]
            );
        }
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
fn observed_units_distinguish_effective_features_outputs_and_repeated_execution() {
    for (case, fails) in [
        ("complete-clean", false),
        ("incomplete", true),
        ("rejected", true),
        ("mixed-rejected", true),
        ("identical", true),
        ("proc-so", true),
        ("proc-dylib", true),
        ("proc-dll", true),
        ("proc-distinct", false),
        ("proc-unhashed", false),
        ("proc-outside", false),
        ("dynamic", false),
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
        if let Some(extension) = case.strip_prefix("proc-") {
            value["invocations"][0]["artifacts"][0]["target"]["kind"] = json!(["proc-macro"]);
            value["invocations"][0]["artifacts"][0]["target"]["crate_types"] =
                json!(["proc-macro"]);
            let filename = match extension {
                "unhashed" => "target/debug/deps/libserde.dylib".to_owned(),
                "outside" => "target/debug/libserde-opaque.dylib".to_owned(),
                "distinct" => "target/debug/deps/libserde-opaque.dylib".to_owned(),
                extension => format!("target/debug/deps/libserde-opaque.{extension}"),
            };
            value["invocations"][0]["artifacts"][0]["filenames"] = json!([filename]);
        } else if case == "dynamic" {
            value["invocations"][0]["artifacts"][0]["filenames"] =
                json!(["target/debug/deps/libserde-opaque.dylib"]);
        }
        if matches!(case, "complete-clean" | "incomplete" | "rejected") {
            value["invocations"][0]["artifacts"] = json!([]);
        }
        let first = metrics(value.clone());
        value["lane"] = json!("second");
        vary_unit(&mut value, case);
        let second = metrics(value.clone());
        let result = check(&[first, second.clone()]);
        assert_eq!(result.is_err(), fails, "{case}: {result:?}");
        if fails {
            assert!(result.err().is_some_and(|error| error.to_string().contains(
                if matches!(case, "incomplete" | "rejected" | "mixed-rejected") {
                    "cannot evaluate"
                } else {
                    "serde"
                }
            )));
        }
        if !fails {
            value["lane"] = json!("third");
            if case != "complete-clean" {
                value["invocations"][0]["artifacts"][0]["fresh"] = json!(true);
            }
            assert!(check(&[second, metrics(value)]).is_ok(), "{case}");
        }
    }
}

fn vary_unit(value: &mut Value, case: &str) {
    match case {
        "incomplete" => value["invocations"][0]["capture_complete"] = json!(false),
        "rejected" | "mixed-rejected" => {
            value["invocations"][0]["rejected"] = json!(1);
            if case == "rejected" {
                value["invocations"][0]["artifacts"] = json!([]);
            }
        }
        "proc-distinct" => {
            value["invocations"][0]["artifacts"][0]["filenames"] =
                json!(["target/debug/deps/libserde-other.dylib"]);
        }
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
}
