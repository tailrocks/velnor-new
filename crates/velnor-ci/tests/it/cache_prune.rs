// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use serde_json::{Value, json};

use crate::io::Repo as Checkout;

const SERDE: &str = "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0";
const OLD: &str = "registry+https://github.com/rust-lang/crates.io-index#old@1.0.0";
const APP: &str = "path+file:///work/app#app@0.1.0";
const SERDE_HASH: &str = "aaaaaaaaaaaaaaaa";
const SERDE_SCRIPT_HASH: &str = "bbbbbbbbbbbbbbbb";
const SERDE_RUN_HASH: &str = "cccccccccccccccc";
const APP_HASH: &str = "dddddddddddddddd";
const OLD_HASH: &str = "eeeeeeeeeeeeeeee";
const DEBUG: &str = "target/debug";

fn artifact(package: &str, kind: &str, file: &str) -> Value {
    let mut record = crate::cargo_artifacts::record(false);
    record["package_id"] = json!(package);
    record["target"]["kind"] = json!([kind]);
    record["filenames"] = json!([file]);
    record
}

fn invocation(complete: bool) -> Value {
    json!({
        "argv": ["cargo", "build"], "root": ".", "configuration": "default",
        "started_at": 0, "seconds": 0.0, "log_start": 0, "log_end": 0,
        "compilations": [], "downloads": [], "capture_complete": complete,
        "progress": [{"kind": "Compiling", "package": "serde", "version": "1.0.0", "location": null}],
        "artifacts": [
            artifact(SERDE, "lib", &format!("/work/{DEBUG}/deps/libserde-{SERDE_HASH}.rlib")),
            artifact(SERDE, "lib", &format!("/work/{DEBUG}/deps/libserde-{SERDE_HASH}.rmeta")),
            artifact(SERDE, "custom-build", &format!("/work/{DEBUG}/build/serde-{SERDE_SCRIPT_HASH}/build_script_build-{SERDE_SCRIPT_HASH}")),
            artifact(APP, "lib", &format!("/work/{DEBUG}/deps/libapp-{APP_HASH}.rlib")),
        ],
        "build_scripts": [
            {"package_id": SERDE, "out_dir": format!("/work/{DEBUG}/build/serde-{SERDE_RUN_HASH}/out")},
            {"package_id": APP, "out_dir": format!("/work/{DEBUG}/build/app-{APP_HASH}/out")},
        ],
    })
}

fn seed(repo: &Checkout) {
    repo.write_unstaged("Cargo.lock", "version = 4\n");
    let files = [
        // dependency units this run used
        format!("deps/libserde-{SERDE_HASH}.rlib"),
        format!("deps/libserde-{SERDE_HASH}.rmeta"),
        format!("deps/serde-{SERDE_HASH}.d"),
        format!(".fingerprint/serde-{SERDE_HASH}/lib-serde.json"),
        format!(".fingerprint/serde-{SERDE_SCRIPT_HASH}/build-script-build-script-build.json"),
        format!(".fingerprint/serde-{SERDE_RUN_HASH}/run-build-script.json"),
        format!("build/serde-{SERDE_SCRIPT_HASH}/build_script_build-{SERDE_SCRIPT_HASH}"),
        format!("build/serde-{SERDE_RUN_HASH}/output"),
        format!("build/serde-{SERDE_RUN_HASH}/out/generated.rs"),
        "deps/libunhashed.rlib".to_owned(),
        // workspace artifacts, executables and units no cargo command of this run used
        format!("deps/libapp-{APP_HASH}.rlib"),
        format!("deps/libapp-{APP_HASH}.rmeta"),
        format!("deps/app-{APP_HASH}"),
        format!(".fingerprint/app-{APP_HASH}/lib-app.json"),
        format!("build/app-{APP_HASH}/output"),
        format!("deps/libold-{OLD_HASH}.rlib"),
        format!(".fingerprint/old-{OLD_HASH}/lib-old.json"),
        format!("build/old-{OLD_HASH}/output"),
        "app".to_owned(),
        "app.d".to_owned(),
        "libapp.rlib".to_owned(),
        "examples/demo".to_owned(),
        "incremental/app-1/work".to_owned(),
    ];
    for file in files {
        repo.write_unstaged(&format!("{DEBUG}/{file}"), "x");
    }
    repo.write_unstaged("target/debug/.cargo-lock", "");
    repo.write_unstaged("target/CACHEDIR.TAG", "tag");
    repo.write_unstaged("target/.rustc_info.json", "{}");
    repo.write_unstaged("target/doc/app/index.html", "docs");
    repo.write_unstaged("target/nextest/ci/junit.xml", "xml");
    repo.write_unstaged("target/tmp/scratch", "tmp");
    repo.write_unstaged("target/x86_64-unknown-linux-gnu/doc/app/index.html", "docs");
}

fn kept_dependency_units() -> Vec<String> {
    [
        format!("deps/libserde-{SERDE_HASH}.rlib"),
        format!("deps/libserde-{SERDE_HASH}.rmeta"),
        format!("deps/serde-{SERDE_HASH}.d"),
        format!(".fingerprint/serde-{SERDE_HASH}/lib-serde.json"),
        format!(".fingerprint/serde-{SERDE_SCRIPT_HASH}/build-script-build-script-build.json"),
        format!(".fingerprint/serde-{SERDE_RUN_HASH}/run-build-script.json"),
        format!("build/serde-{SERDE_SCRIPT_HASH}/build_script_build-{SERDE_SCRIPT_HASH}"),
        format!("build/serde-{SERDE_RUN_HASH}/output"),
        format!("build/serde-{SERDE_RUN_HASH}/out/generated.rs"),
        "deps/libunhashed.rlib".to_owned(),
        ".cargo-lock".to_owned(),
    ]
    .into_iter()
    .map(|path| format!("{DEBUG}/{path}"))
    .collect()
}

fn decide(repo: &Checkout, records: &[Value], budget: Option<&str>) -> (Output, String) {
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(temp.join("velnor-run.log"), "   Compiling serde v1.0.0\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let lines: Vec<_> = records.iter().map(Value::to_string).collect();
    if !lines.is_empty() {
        fs::write(
            temp.join("velnor-invocations.jsonl"),
            lines.join("\n") + "\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let outputs = repo.root().join("github-output");
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["cache", "decide", "--target-dir", "target"])
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .current_dir(repo.root());
    if let Some(budget) = budget {
        command.env("VELNOR_CACHE_BUDGET_BYTES", budget);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    (output, fs::read_to_string(&outputs).unwrap_or_default())
}

fn exists(repo: &Checkout, path: &str) -> bool {
    repo.root().join(path).exists()
}

#[test]
fn a_saved_build_directory_keeps_only_the_dependency_units_the_run_used() {
    let repo = Checkout::new("prune-used-units");
    seed(&repo);
    let (output, written) = decide(&repo, &[invocation(true)], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=true\nsources=true\n");
    for kept in kept_dependency_units() {
        assert!(exists(&repo, &kept), "{kept} must stay\n{stdout}");
    }
    for gone in [
        format!("deps/libapp-{APP_HASH}.rlib"),
        format!("deps/libapp-{APP_HASH}.rmeta"),
        format!("deps/app-{APP_HASH}"),
        format!(".fingerprint/app-{APP_HASH}"),
        format!("build/app-{APP_HASH}"),
        format!("deps/libold-{OLD_HASH}.rlib"),
        format!(".fingerprint/old-{OLD_HASH}"),
        format!("build/old-{OLD_HASH}"),
        "app".to_owned(),
        "app.d".to_owned(),
        "libapp.rlib".to_owned(),
        "examples".to_owned(),
        "incremental".to_owned(),
    ] {
        assert!(
            !exists(&repo, &format!("{DEBUG}/{gone}")),
            "{gone} must go\n{stdout}"
        );
    }
    for gone in [
        "target/doc",
        "target/nextest",
        "target/tmp",
        "target/x86_64-unknown-linux-gnu/doc",
    ] {
        assert!(!exists(&repo, gone), "{gone} must go");
    }
    assert!(exists(&repo, "target/CACHEDIR.TAG") && exists(&repo, "target/.rustc_info.json"));
    assert!(stdout.contains("pruned from"), "{stdout}");
    assert!(stdout.contains("unused or workspace artifacts"), "{stdout}");
}

#[test]
fn the_size_budget_is_judged_after_the_prune() {
    let repo = Checkout::new("prune-before-budget");
    seed(&repo);
    repo.write_unstaged(
        &format!("{DEBUG}/deps/libapp-{APP_HASH}.rlib"),
        &"w".repeat(50_000),
    );
    repo.write_unstaged("target/doc/app/big.html", &"d".repeat(50_000));
    let (output, written) = decide(&repo, &[invocation(true)], Some("10000"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=true\nsources=true\n", "{stdout}");
    let (output, written) = decide(&repo, &[invocation(true)], Some("10"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(written, "save=false\nsources=true\n", "{stdout}");
    assert!(
        stdout.contains("more than the repository cache budget of 10 bytes"),
        "{stdout}"
    );
    assert!(stdout.contains("pruned from"), "{stdout}");
}

#[test]
fn an_incomplete_record_of_the_run_keeps_every_unit_but_still_drops_what_is_never_reused() {
    let repo = Checkout::new("prune-incomplete");
    seed(&repo);
    let (output, written) = decide(&repo, &[invocation(false)], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=true\nsources=true\n");
    for kept in [
        format!("deps/libold-{OLD_HASH}.rlib"),
        format!(".fingerprint/old-{OLD_HASH}"),
        format!("deps/libapp-{APP_HASH}.rlib"),
        format!("build/old-{OLD_HASH}"),
    ] {
        assert!(
            exists(&repo, &format!("{DEBUG}/{kept}")),
            "{kept}\n{stdout}"
        );
    }
    for gone in [
        format!("deps/app-{APP_HASH}"),
        "app".to_owned(),
        "examples".to_owned(),
    ] {
        assert!(!exists(&repo, &format!("{DEBUG}/{gone}")), "{gone}");
    }
    assert!(!exists(&repo, "target/doc") && !exists(&repo, "target/nextest"));
}

#[test]
fn a_run_with_no_record_of_its_units_prunes_without_the_unit_list() {
    let repo = Checkout::new("prune-no-record");
    seed(&repo);
    let (output, written) = decide(&repo, &[], None);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n");
    assert!(exists(
        &repo,
        &format!("{DEBUG}/deps/libold-{OLD_HASH}.rlib")
    ));
    assert!(!exists(&repo, "target/doc"));
    assert!(!exists(&repo, &format!("{DEBUG}/deps/app-{APP_HASH}")));
    assert!(String::from_utf8_lossy(&output.stdout).contains("no complete record"));
}
