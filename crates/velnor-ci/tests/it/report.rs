// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::report::{LaneMetrics, check, restore_notices, waste};
use velnor_ci::telemetry::configuration;

mod fallback;
mod restored_units;

pub(crate) fn lane() -> Value {
    json!({"schema":1,"lane":"rust","caches":[{"family":"target","root":".","restore_step":"target_root","save_step":"save_target_root","requested_key":"target-lock-new","matched_key":"target-lock-old","hit":"fallback","bytes":null,"usable":true,"usable_same_lock":true,"save":"not_attempted"}],"invocations":[{"argv":["cargo","check"],"root":".","configuration":"default","started_at":10,"seconds":1.0,"log_start":0,"log_end":0,"compilations":[{"package":"serde","version":"1.0.0","workspace":false,"unchanged":false,"dependencies":[]}],"downloads":[],"capture_complete":true,"verified_git_downloads":[]}],"downloaded":0,"first_cargo_seconds":null,"slowest_tests":[],"tests_run":null,"diagnostics":[],"violations":[]})
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
        ("shared-sources-download", true),
        ("save-failed", true),
    ] {
        let mut value = lane();
        match case {
            "cold" => {
                value["caches"][0]["usable"] = json!(false);
                value["caches"][0]["usable_same_lock"] = json!(false);
            }
            "other-lock" => value["caches"][0]["usable_same_lock"] = json!(false),
            "dependency" => {
                value["invocations"][0]["compilations"][0]["restored"] = json!(true);
            }
            "workspace" | "changed" | "dependency-compiled" => {
                value["invocations"][0]["compilations"][0]["workspace"] = json!(true);
                value["invocations"][0]["compilations"][0]["unchanged"] = json!(case != "changed");
                if case == "dependency-compiled" {
                    value["invocations"][0]["compilations"][0]["dependencies"] = json!(["serde"]);
                }
            }
            "download" | "other-root-download" | "shared-sources-download" => {
                value["caches"][0]["family"] = json!("sources");
                value["caches"][0]["hit"] = json!("exact");
                value["invocations"][0]["downloads"] = json!(["serde v1.0.0"]);
                if case == "other-root-download" {
                    value["caches"][0]["root"] = json!("tools");
                }
                if case == "shared-sources-download" {
                    value["caches"][0]["root"] = json!(null);
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

fn artifact(package_id: &str, fresh: bool, filenames: &[&str]) -> Value {
    artifact_kind(package_id, fresh, &["lib"], filenames)
}

fn artifact_kind(package_id: &str, fresh: bool, kind: &[&str], filenames: &[&str]) -> Value {
    json!({
        "package_id": package_id,
        "manifest_path": "/checkout/app/Cargo.toml",
        "target": {"name": "app", "kind": kind, "crate_types": ["lib"], "src_path": "/checkout/app/src/lib.rs"},
        "profile": {"opt_level": "0", "debuginfo": 0, "debug_assertions": true, "overflow_checks": true, "test": false},
        "features": [],
        "filenames": filenames,
        "fresh": fresh
    })
}

fn workspace_app(artifacts: Value, target_directory: Option<&str>) -> Value {
    let mut value = lane();
    value["invocations"][0]["compilations"][0]["package"] = json!("app");
    value["invocations"][0]["compilations"][0]["workspace"] = json!(true);
    value["invocations"][0]["compilations"][0]["unchanged"] = json!(true);
    value["invocations"][0]["artifacts"] = artifacts;
    if let Some(directory) = target_directory {
        value["invocations"][0]["target_directory"] = json!(directory);
    }
    value
}

#[test]
fn pruned_doc_and_executable_outputs_are_not_workspace_waste() {
    let mut value = lane();
    value["invocations"][0]["compilations"][0]["package"] = json!("app");
    value["invocations"][0]["compilations"][0]["workspace"] = json!(true);
    value["invocations"][0]["compilations"][0]["unchanged"] = json!(true);
    value["invocations"][0]["artifacts"] = json!([
        artifact(
            "path+file:///checkout/app#1.0.0",
            false,
            &["target/doc/app/index.html"]
        ),
        artifact(
            "path+file:///checkout/app#1.0.0",
            false,
            &["target/debug/app"]
        ),
        artifact(
            "path+file:///checkout/app#1.0.0",
            false,
            &["target/debug/build/app/aaaaaaaaaaaaaaaa/out/app-aaaaaaaaaaaaaaaa"]
        ),
        artifact(
            "path+file:///checkout/app#1.0.0",
            true,
            &["target/debug/deps/libapp-bbbbbbbbbbbbbbbb.rlib"]
        ),
    ]);
    assert_eq!(waste(&metrics(value)), [] as [&str; 0]);

    let mut value = lane();
    value["invocations"][0]["compilations"][0]["package"] = json!("app");
    value["invocations"][0]["compilations"][0]["workspace"] = json!(true);
    value["invocations"][0]["compilations"][0]["unchanged"] = json!(true);
    value["invocations"][0]["artifacts"] = json!([
        artifact(
            "path+file:///checkout/app#1.0.0",
            false,
            &["target/doc/app/index.html"]
        ),
        artifact(
            "path+file:///checkout/app#1.0.0",
            false,
            &["target/debug/deps/libapp-aaaaaaaaaaaaaaaa.rlib"]
        ),
    ]);
    assert_eq!(
        waste(&metrics(value)),
        [".: app v1.0.0 compiled despite usable restored inputs"]
    );
}

#[test]
fn pruned_output_classification_keeps_real_library_rebuilds() {
    let waste_line = [".: app v1.0.0 compiled despite usable restored inputs"];
    let under_doc_checkout = workspace_app(
        json!([artifact(
            "path+file:///srv/doc/project/app#1.0.0",
            false,
            &["/srv/doc/project/target/debug/deps/libapp-aaaaaaaaaaaaaaaa.rlib"]
        )]),
        Some("/srv/doc/project/target"),
    );
    assert_eq!(waste(&metrics(under_doc_checkout)), waste_line);

    let windows_doc = workspace_app(
        json!([artifact(
            "path+file:///D:/a/repo/app#1.0.0",
            false,
            &["D:\\a\\repo\\target\\doc\\app\\index.html"]
        )]),
        Some("D:\\a\\repo\\target"),
    );
    assert_eq!(waste(&metrics(windows_doc)), [] as [&str; 0]);

    let build_script = workspace_app(
        json!([artifact_kind(
            "path+file:///checkout/app#1.0.0",
            false,
            &["custom-build"],
            &["target/debug/build/app-aaaaaaaaaaaaaaaa/build_script_build-aaaaaaaaaaaaaaaa"]
        )]),
        None,
    );
    assert_eq!(waste(&metrics(build_script)), waste_line);

    let same_name = workspace_app(
        json!([
            artifact(
                "path+file:///checkout/app#1.0.0",
                false,
                &["target/debug/app"]
            ),
            artifact(
                "registry+https://github.com/rust-lang/crates.io-index#app@1.0.0",
                false,
                &["target/debug/deps/libapp-aaaaaaaaaaaaaaaa.rlib"]
            )
        ]),
        None,
    );
    assert_eq!(waste(&metrics(same_name)), [] as [&str; 0]);
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
