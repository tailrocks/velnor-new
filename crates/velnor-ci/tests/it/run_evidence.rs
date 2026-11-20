// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;
use velnor_ci::telemetry::Invocation;

#[test]
fn toolchain_identity_uses_only_the_first_argument_and_preserves_config_values() {
    let temp = std::env::temp_dir().join(format!("velnor-toolchain-{}", std::process::id()));
    let mut units = BTreeMap::new();
    let mut baseline = None;
    for args in [
        vec!["check", "--target-dir", "+path", "--config", "+config"],
        vec!["check", "--target-dir=+path", "--config=+config"],
        vec![
            "+nightly",
            "check",
            "--target-dir",
            "+path",
            "--config",
            "+config",
        ],
    ] {
        let program = crate::run_capture::fixture(&temp, true, false).expect("fixture");
        fs::write(&program, "#!/bin/sh\ncat events\n").expect("program");
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["run", "--"])
            .arg(&program)
            .args(&args)
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp)
            .env("CARGO_HOME", temp.join("cargo-home"))
            .output()
            .expect("capture");
        assert!(output.status.success(), "{output:?}");
        let text = fs::read_to_string(temp.join("velnor-invocations.jsonl")).expect("record");
        let invocation: Invocation = serde_json::from_str(text.trim()).expect("invocation");
        assert_eq!(&invocation.argv[1..], args);
        assert_eq!(
            invocation.execution["cargo-config-arguments"],
            "[\"+config\"]"
        );
        assert_eq!(
            invocation
                .execution
                .get("toolchain-argument")
                .map(String::as_str),
            (args[0] == "+nightly").then_some("+nightly")
        );
        if args[0] == "check" {
            if let Some(baseline) = &baseline {
                assert_eq!(&invocation.execution, baseline);
            } else {
                baseline = Some(invocation.execution.clone());
            }
        }
        velnor_ci::observed_units::collect(&invocation, &mut units);
    }
    assert_eq!(units.len(), 2);
    assert_eq!(units.values().filter(|(_, count)| *count == 2).count(), 1);
    fs::remove_dir_all(temp).expect("cleanup");
}

#[test]
fn quiet_nonfresh_artifacts_save_the_cache_without_counting_verbose_artifacts_twice() {
    for (name, fresh, verbose) in [
        ("quiet", false, false),
        ("fresh", true, false),
        ("verbose", false, true),
        ("targets", false, false),
    ] {
        let temp = crate::scratch_path("velnor-quiet", name);
        let program = crate::run_capture::fixture(&temp, true, false).expect("fixture");
        fs::write(&program, "#!/bin/sh\ncat events\n").expect("program");
        fs::write(
            temp.join("events"),
            format!(
                "{}{}\n",
                if verbose {
                    "Compiling app v0.1.0\n"
                } else {
                    ""
                },
                crate::cargo_artifacts::record(fresh)
            ),
        )
        .expect("events");
        if name == "targets" {
            use std::io::Write;
            let mut test = crate::cargo_artifacts::record(false);
            test["target"]["name"] = serde_json::json!("app_integration");
            test["target"]["kind"] = serde_json::json!(["test"]);
            test["profile"]["test"] = serde_json::json!(true);
            test["filenames"] = serde_json::json!(["target/debug/deps/app_integration-opaque"]);
            writeln!(
                fs::OpenOptions::new()
                    .append(true)
                    .open(temp.join("events"))
                    .expect("events"),
                "{test}"
            )
            .expect("test artifact");
        }
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["run", "--"])
            .arg(&program)
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp)
            .output()
            .expect("capture");
        assert!(output.status.success(), "{output:?}");
        let decision = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["cache", "decide", "--target-dir", "target"])
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp)
            .env("GITHUB_OUTPUT", temp.join("outputs"))
            .output()
            .expect("decision");
        assert!(decision.status.success(), "{decision:?}");
        assert_eq!(
            fs::read_to_string(temp.join("outputs")).expect("outputs"),
            format!("save={}\nsources=true\n", !fresh),
            "{name}"
        );
        if !fresh {
            assert!(
                String::from_utf8_lossy(&decision.stdout).starts_with("1 crates compiled"),
                "{decision:?}"
            );
        }
        fs::remove_dir_all(temp).expect("cleanup");
    }
}

#[test]
fn cargo_build_compiler_selectors_are_recorded_and_distinguish_compilation_identity() {
    let selectors = [
        "CARGO_BUILD_RUSTC",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    ];
    let temp = std::env::temp_dir().join(format!("velnor-selectors-{}", std::process::id()));
    let mut units = BTreeMap::new();
    let mut baseline = None;
    for changed in [
        None,
        Some(selectors[0]),
        Some(selectors[1]),
        Some(selectors[2]),
        Some("UNRELATED"),
    ] {
        let program = crate::run_capture::fixture(&temp, true, false).expect("fixture");
        fs::write(&program, "#!/bin/sh\ncat events\n").expect("program");
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .args(["run", "--"])
            .arg(&program)
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp);
        for selector in selectors {
            command.env(
                selector,
                if changed == Some(selector) {
                    "changed"
                } else {
                    "baseline"
                },
            );
        }
        command.env(
            "UNRELATED",
            if changed == Some("UNRELATED") {
                "changed"
            } else {
                "baseline"
            },
        );
        let output = command.output().expect("capture");
        assert!(output.status.success(), "{output:?}");
        let text = fs::read_to_string(temp.join("velnor-invocations.jsonl")).expect("record");
        let invocation: Invocation = serde_json::from_str(text.trim()).expect("invocation");
        for selector in selectors {
            assert_eq!(
                invocation.execution.get(selector).map(String::as_str),
                Some(if changed == Some(selector) {
                    "changed"
                } else {
                    "baseline"
                })
            );
        }
        if changed == Some("UNRELATED") {
            assert_eq!(baseline.as_ref(), Some(&invocation.execution));
        } else if changed.is_none() {
            baseline = Some(invocation.execution.clone());
        }
        velnor_ci::observed_units::collect(&invocation, &mut units);
    }
    assert_eq!(units.len(), 4);
    assert_eq!(units.values().filter(|(_, count)| *count == 2).count(), 1);
    fs::remove_dir_all(temp).expect("cleanup");
}

#[test]
fn non_unicode_compiler_selectors_are_rejected_before_execution_but_unrelated_values_work() {
    use std::os::unix::ffi::OsStringExt;
    let temp = std::env::temp_dir().join(format!("velnor-selector-bytes-{}", std::process::id()));
    for key in ["CARGO_BUILD_RUSTC", "UNRELATED"] {
        for byte in [0xfe, 0xff] {
            let program = crate::run_capture::fixture(&temp, false, false).expect("fixture");
            fs::write(&program, "#!/bin/sh\ntouch executed\n").expect("program");
            let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
                .args(["run", "--"])
                .arg(program)
                .current_dir(&temp)
                .env("RUNNER_TEMP", &temp)
                .env(key, std::ffi::OsString::from_vec(vec![byte]))
                .output()
                .expect("capture");
            let recognized = key == "CARGO_BUILD_RUSTC";
            assert_eq!(output.status.success(), !recognized, "{key}: {output:?}");
            assert_eq!(temp.join("executed").exists(), !recognized);
            if recognized {
                let error = String::from_utf8_lossy(&output.stderr);
                assert!(error.contains(key) && error.contains("Unicode"), "{error}");
            }
        }
    }
    fs::remove_dir_all(temp).expect("cleanup");
}

#[test]
fn rejected_oversized_output_cannot_establish_zero_cache_activity() {
    for active in [false, true] {
        let temp = crate::scratch_path("velnor-oversized", &active.to_string());
        let program = crate::run_capture::fixture(&temp, false, false).expect("fixture");
        fs::write(&program, "#!/bin/sh\ncat events\n").expect("program");
        let mut artifact = crate::cargo_artifacts::record(false);
        artifact["filenames"] = serde_json::json!(["x".repeat(1024 * 1024)]);
        fs::write(
            temp.join("events"),
            format!(
                "{artifact}\n{}",
                if active {
                    format!("{}\n", crate::cargo_artifacts::record(false))
                } else {
                    String::new()
                }
            ),
        )
        .expect("events");
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["run", "--"])
            .arg(program)
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp)
            .output()
            .expect("capture");
        assert!(output.status.success());
        let invocation: Invocation = serde_json::from_str(
            fs::read_to_string(temp.join("velnor-invocations.jsonl"))
                .expect("record")
                .trim(),
        )
        .expect("invocation");
        assert_eq!(invocation.rejected, 1);
        let decision = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["cache", "decide", "--target-dir", "target"])
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp)
            .env("GITHUB_OUTPUT", temp.join("outputs"))
            .output()
            .expect("decision");
        assert_eq!(decision.status.success(), active, "{decision:?}");
        if active {
            assert_eq!(
                fs::read_to_string(temp.join("outputs")).expect("outputs"),
                "save=true\nsources=true\n"
            );
        } else {
            assert!(String::from_utf8_lossy(&decision.stderr).contains("rejected"));
            assert!(!temp.join("outputs").exists());
        }
        fs::remove_dir_all(temp).expect("cleanup");
    }
}
