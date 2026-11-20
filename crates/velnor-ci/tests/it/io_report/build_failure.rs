// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::fs;
use velnor_ci::report::{LaneMetrics, check};

// `serde` compiled (an artifact was emitted); `demo` failed with a compile error
// and no artifact. `build_finished` is cargo's own verdict for the whole build.
fn serde_artifact() -> Value {
    json!({
        "reason":"compiler-artifact",
        "package_id":"registry+https://github.com/rust-lang/crates.io-index#serde@1.0.200",
        "manifest_path":"/registry/serde-1.0.200/Cargo.toml",
        "target":{"name":"serde","kind":["lib"],"crate_types":["lib"],"src_path":"/registry/serde-1.0.200/src/lib.rs"},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":[],"filenames":["target/debug/deps/libserde-hash.rmeta"],"fresh":false
    })
}

fn compile_error() -> Value {
    json!({
        "reason":"compiler-message",
        "package_id":"path+file:///checkout#demo@0.1.0",
        "manifest_path":"/checkout/Cargo.toml",
        "target":{"name":"demo","kind":["lib"],"crate_types":["lib"],"src_path":"/checkout/src/lib.rs"},
        "message":{"level":"error","message":"cannot find value `missing` in this scope","code":null,"spans":[],"children":[],"rendered":"error[E0425]: cannot find value `missing` in this scope\n"}
    })
}

fn build_log(compile_failed: bool, success: bool) -> String {
    let mut lines = vec![
        "   Compiling serde v1.0.200".to_owned(),
        "   Compiling demo v0.1.0".to_owned(),
        serde_artifact().to_string(),
    ];
    if compile_failed {
        lines.push(compile_error().to_string());
    }
    lines.push(json!({"reason":"build-finished","success":success}).to_string());
    if !success {
        lines.push("error: could not compile `demo` (lib) due to 1 previous error".to_owned());
    }
    lines.join("\n") + "\n"
}

fn lane_with_log(name: &str, log: &str) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let repo = super::fixture(name)?;
    let root = fs::canonicalize(repo.root())?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["root"] = json!(root);
    invocation["argv"] = json!([
        "cargo",
        "build",
        "--workspace",
        "--manifest-path",
        root.join("Cargo.toml")
    ]);
    invocation["compilations"] = json!([]);
    invocation["progress"] = Value::Null;
    invocation["log_start"] = json!(0);
    invocation["log_end"] = json!(log.len());
    invocation["artifacts"] = json!([serde_artifact()]);
    fs::write(repo.root().join("velnor-run.log"), log)?;
    fs::write(
        repo.root().join("velnor-invocations.jsonl"),
        format!("{invocation}\n"),
    )?;
    super::report(
        &repo,
        &json!([]),
        r#"{"restore":{"outcome":"skipped","outputs":{}}}"#,
    )
}

#[test]
fn failed_compile_is_skipped_with_a_note_not_a_missing_evidence_failure() -> super::ResultTest {
    let metrics = lane_with_log("report-failed-compile", &build_log(true, false))?;
    assert!(
        !metrics
            .verification_errors
            .iter()
            .any(|message| message.contains("no matching artifact evidence")),
        "{:?}",
        metrics.verification_errors
    );
    assert!(
        metrics.diagnostics.iter().any(|message| message
            .contains("lane rust: build failed for demo; cache waste not evaluated for it")),
        "{:?}",
        metrics.diagnostics
    );
    assert!(check(std::slice::from_ref(&metrics)).is_ok());
    Ok(())
}

#[test]
fn successful_build_with_missing_artifact_evidence_still_fails() -> super::ResultTest {
    let metrics = lane_with_log("report-missing-evidence", &build_log(false, true))?;
    let error = check(std::slice::from_ref(&metrics))
        .err()
        .unwrap_or_else(|| unreachable!("{:?}", metrics.verification_errors));
    assert!(
        error
            .to_string()
            .contains("demo v0.1.0: progress has no matching artifact evidence"),
        "{error}"
    );
    Ok(())
}
