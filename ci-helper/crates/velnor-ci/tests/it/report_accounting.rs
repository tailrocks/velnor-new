// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::report::{LaneMetrics, check};

fn artifact(kind: &str, filename: &str) -> Value {
    json!({
        "package_id":"registry+https://example.invalid/index#serde@1.0.0",
        "manifest_path":"registry/serde/Cargo.toml",
        "target":{"name":"serde","kind":[kind],"crate_types":[kind],"src_path":"registry/serde/src/lib.rs"},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":["default"],"filenames":[filename],"fresh":false
    })
}

fn lane(index: usize, artifacts: Vec<Value>) -> Value {
    let mut value = super::report::lane();
    value["lane"] = json!(format!("lane-{index}"));
    value["caches"] = json!([]);
    value["invocations"][0]["artifacts"] = Value::Array(artifacts);
    value
}

fn metrics(value: Value) -> LaneMetrics {
    serde_json::from_value(value).unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn third_unknown_observation_fails_verification_without_claiming_identical_units() {
    for kind in ["bin", "custom-build"] {
        let lanes: Vec<_> = (0..3)
            .map(|index| metrics(lane(index, vec![artifact(kind, "target/debug/serde")])))
            .collect();
        for count in [1, 2] {
            assert!(check(&lanes[..count]).is_ok(), "{kind}, {count}");
        }
        let error = check(&lanes)
            .err()
            .unwrap_or_else(|| unreachable!("{kind}"));
        let message = error.to_string();
        assert!(message.contains("cannot evaluate cache waste"), "{message}");
        assert!(message.contains("root . crate serde"), "{message}");
        assert!(
            message.contains("compilation identity remains unknown after 3 observations"),
            "{message}"
        );
        assert!(!message.contains("identical observed compilation unit"));
    }
}

#[test]
fn known_same_package_siblings_do_not_exempt_unknown_artifacts() {
    let lanes: Vec<_> = (0..3)
        .map(|index| {
            metrics(lane(
                index,
                vec![
                    artifact("lib", &format!("target/debug/deps/libserde-{index}.rmeta")),
                    artifact("bin", "target/debug/serde"),
                ],
            ))
        })
        .collect();
    assert!(check(&lanes[..2]).is_ok());
    let error = check(&lanes).err().unwrap_or_else(|| unreachable!());
    assert!(error.to_string().contains("identity remains unknown"));
}

#[test]
fn legacy_compilations_require_matching_nonfresh_artifacts_in_the_same_invocation() {
    for case in [
        "prior-same",
        "other-package",
        "other-version",
        "fresh",
        "none",
    ] {
        let mut lanes = Vec::new();
        if case == "prior-same" {
            let mut prior = lane(
                3,
                vec![artifact("lib", "target/debug/deps/libserde-prior.rmeta")],
            );
            prior["invocations"][0]["compilations"] = json!([]);
            lanes.push(metrics(prior));
        }
        for index in 0..3 {
            let mut observed =
                artifact("lib", &format!("target/debug/deps/libserde-{index}.rmeta"));
            match case {
                "other-package" => {
                    observed["package_id"] =
                        json!("registry+https://example.invalid/index#other@1.0.0");
                }
                "other-version" => {
                    observed["package_id"] =
                        json!("registry+https://example.invalid/index#serde@2.0.0");
                }
                "fresh" => observed["fresh"] = json!(true),
                _ => {}
            }
            let artifacts = if matches!(case, "prior-same" | "none") {
                vec![]
            } else {
                vec![observed]
            };
            lanes.push(metrics(lane(index, artifacts)));
        }
        assert!(check(&lanes[..lanes.len() - 1]).is_ok(), "{case}");
        let error = check(&lanes)
            .err()
            .unwrap_or_else(|| unreachable!("{case}"));
        assert!(
            error.to_string().contains("serde v1.0.0 compiled 3 times"),
            "{case}: {error}"
        );
    }
}

#[test]
fn distinct_unknown_available_contexts_remain_separate() {
    for dimension in [
        "root",
        "configuration",
        "package",
        "target",
        "profile",
        "features",
        "filenames",
        "execution",
    ] {
        let lanes: Vec<_> = (0..3)
            .map(|index| {
                let mut value = lane(index, vec![artifact("bin", "target/debug/serde")]);
                let invocation = &mut value["invocations"][0];
                match dimension {
                    "root" => invocation["root"] = json!(format!("root-{index}")),
                    "configuration" => {
                        invocation["configuration"] = json!(format!("config-{index}"));
                    }
                    "package" => {
                        invocation["artifacts"][0]["package_id"] = json!(format!(
                            "registry+https://example.invalid/index#serde@1.0.{index}"
                        ));
                    }
                    "target" => {
                        invocation["artifacts"][0]["target"]["name"] =
                            json!(format!("serde-{index}"));
                    }
                    "profile" => {
                        invocation["artifacts"][0]["profile"]["opt_level"] =
                            json!(index.to_string());
                    }
                    "features" => {
                        invocation["artifacts"][0]["features"] =
                            json!([format!("feature-{index}")]);
                    }
                    "filenames" => {
                        invocation["artifacts"][0]["filenames"] =
                            json!([format!("target/debug/serde-{index}")]);
                    }
                    "execution" => {
                        invocation["execution"] =
                            json!({"RUSTFLAGS":format!("-Ccodegen-units={index}")});
                    }
                    _ => unreachable!(),
                }
                invocation["compilations"] = json!([]);
                metrics(value)
            })
            .collect();
        assert!(check(&lanes).is_ok(), "{dimension}");
    }
}

#[test]
fn unknown_context_features_and_filenames_are_order_independent() {
    let lanes: Vec<_> = (0..3)
        .map(|index| {
            let mut observed = artifact("bin", "target/debug/serde");
            observed["features"] = if index == 1 {
                json!(["b", "a", "a"])
            } else {
                json!(["a", "b"])
            };
            observed["filenames"] = if index == 1 {
                json!(["target/debug/b", "target/debug/a", "target/debug/a"])
            } else {
                json!(["target/debug/a", "target/debug/b"])
            };
            metrics(lane(index, vec![observed]))
        })
        .collect();
    assert!(check(&lanes[..2]).is_ok());
    assert!(check(&lanes).is_err());
}

#[test]
fn distinct_known_unit_features_profiles_and_outputs_do_not_repeat() {
    for dimension in ["features", "profile", "output"] {
        let lanes: Vec<_> = (0..3)
            .map(|index| {
                let mut observed = artifact("lib", "target/debug/deps/libserde-unit.rmeta");
                match dimension {
                    "features" => observed["features"] = json!([format!("feature-{index}")]),
                    "profile" => observed["profile"]["opt_level"] = json!(index.to_string()),
                    "output" => {
                        observed["filenames"] =
                            json!([format!("target/debug/deps/libserde-{index}.rmeta")]);
                    }
                    _ => unreachable!(),
                }
                metrics(lane(index, vec![observed]))
            })
            .collect();
        assert!(check(&lanes).is_ok(), "{dimension}");
    }
}
