// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The prune of a build directory in the layout Cargo 1.100 writes, `build/<package>/<hash>`,
//! with a unit directory per unit instead of `.fingerprint/`, `deps/` and `build/<package>-<hash>`.

use std::fs;

use serde_json::{Value, json};

use crate::cache_prune::{
    APP, APP_HASH, DEBUG, OLD_HASH, SERDE, SERDE_HASH, SERDE_RUN_HASH, SERDE_SCRIPT_HASH, artifact,
    decide, exists,
};
use crate::io::Repo as Checkout;

fn invocation() -> Value {
    json!({
        "argv": ["cargo", "build"], "root": ".", "configuration": "default",
        "started_at": 0, "seconds": 0.0, "log_start": 0, "log_end": 0,
        "compilations": [], "downloads": [], "capture_complete": true,
        "progress": [{"kind": "Compiling", "package": "serde", "version": "1.0.0", "location": null}],
        "artifacts": [
            artifact(SERDE, "lib", &format!("/work/{DEBUG}/build/serde/{SERDE_HASH}/out/libserde-{SERDE_HASH}.rlib")),
            artifact(SERDE, "lib", &format!("/work/{DEBUG}/build/serde/{SERDE_HASH}/out/libserde-{SERDE_HASH}.rmeta")),
            artifact(SERDE, "custom-build", &format!("/work/{DEBUG}/build/serde/{SERDE_SCRIPT_HASH}/out/build_script_build")),
            artifact(APP, "lib", &format!("/work/{DEBUG}/build/app/{APP_HASH}/out/libapp-{APP_HASH}.rlib")),
        ],
        "build_scripts": [
            {"package_id": SERDE, "out_dir": format!("/work/{DEBUG}/build/serde/{SERDE_RUN_HASH}/out")},
        ],
    })
}

fn seed(repo: &Checkout) {
    repo.write_unstaged("Cargo.lock", "version = 4\n");
    let used = [
        format!("build/serde/{SERDE_HASH}/out/libserde-{SERDE_HASH}.rlib"),
        format!("build/serde/{SERDE_HASH}/out/libserde-{SERDE_HASH}.rmeta"),
        format!("build/serde/{SERDE_HASH}/out/serde-{SERDE_HASH}.d"),
        format!("build/serde/{SERDE_HASH}/fingerprint/lib-serde.json"),
        format!("build/serde/{SERDE_SCRIPT_HASH}/out/build_script_build"),
        format!("build/serde/{SERDE_SCRIPT_HASH}/out/build_script_build.d"),
        format!("build/serde/{SERDE_SCRIPT_HASH}/fingerprint/build-script-build-script-build.json"),
        format!("build/serde/{SERDE_RUN_HASH}/run/stdout"),
        format!("build/serde/{SERDE_RUN_HASH}/out/generated.rs"),
        format!(
            "build/serde/{SERDE_RUN_HASH}/fingerprint/run-build-script-build-script-build.json"
        ),
    ];
    let dropped = [
        format!("build/serde/{SERDE_HASH}/out/serde-test"),
        format!("build/app/{APP_HASH}/out/libapp-{APP_HASH}.rlib"),
        format!("build/app/{APP_HASH}/fingerprint/lib-app.json"),
        format!("build/old/{OLD_HASH}/out/libold-{OLD_HASH}.rlib"),
        format!("build/old/{OLD_HASH}/fingerprint/lib-old.json"),
        format!("build/old/{OLD_HASH}/run/stdout"),
        "app".to_owned(),
        "app.d".to_owned(),
        "examples/demo".to_owned(),
        "incremental/app-1/work".to_owned(),
    ];
    for file in used.iter().chain(&dropped) {
        repo.write_unstaged(&format!("{DEBUG}/{file}"), "x");
    }
    for lock in [".cargo-build-lock", ".cargo-artifact-lock", ".cargo-lock"] {
        repo.write_unstaged(&format!("{DEBUG}/{lock}"), "");
    }
    repo.write_unstaged("target/CACHEDIR.TAG", "tag");
    repo.write_unstaged("target/.rustc_info.json", "{}");
    repo.write_unstaged("target/doc/app/index.html", "docs");
}

#[test]
fn a_saved_build_directory_keeps_only_the_dependency_units_the_run_used() {
    let repo = Checkout::new("prune-1100-used-units");
    seed(&repo);
    let (output, written) = decide(&repo, &[invocation()], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=true\nsources=true\n", "{stdout}");
    for kept in [
        format!("build/serde/{SERDE_HASH}/out/libserde-{SERDE_HASH}.rlib"),
        format!("build/serde/{SERDE_HASH}/out/libserde-{SERDE_HASH}.rmeta"),
        format!("build/serde/{SERDE_HASH}/out/serde-{SERDE_HASH}.d"),
        format!("build/serde/{SERDE_HASH}/fingerprint/lib-serde.json"),
        format!("build/serde/{SERDE_SCRIPT_HASH}/out/build_script_build"),
        format!("build/serde/{SERDE_SCRIPT_HASH}/fingerprint/build-script-build-script-build.json"),
        format!("build/serde/{SERDE_RUN_HASH}/run/stdout"),
        format!("build/serde/{SERDE_RUN_HASH}/out/generated.rs"),
        format!(
            "build/serde/{SERDE_RUN_HASH}/fingerprint/run-build-script-build-script-build.json"
        ),
        ".cargo-build-lock".to_owned(),
        ".cargo-artifact-lock".to_owned(),
    ] {
        assert!(
            exists(&repo, &format!("{DEBUG}/{kept}")),
            "{kept} must stay\n{stdout}"
        );
    }
    for gone in [
        format!("build/serde/{SERDE_HASH}/out/serde-test"),
        format!("build/app/{APP_HASH}"),
        format!("build/old/{OLD_HASH}"),
        "app".to_owned(),
        "app.d".to_owned(),
        "examples".to_owned(),
        "incremental".to_owned(),
    ] {
        assert!(
            !exists(&repo, &format!("{DEBUG}/{gone}")),
            "{gone} must go\n{stdout}"
        );
    }
    assert!(!exists(&repo, "target/doc"));
    assert!(stdout.contains("pruned from"), "{stdout}");
    assert!(stdout.contains("unused or workspace artifacts"), "{stdout}");
    assert!(stdout.contains("fingerprints"), "{stdout}");
}

#[test]
fn a_run_with_no_record_of_its_units_keeps_every_unit_directory() {
    let repo = Checkout::new("prune-1100-no-record");
    seed(&repo);
    let (output, written) = decide(&repo, &[], None);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n");
    for kept in [
        format!("build/old/{OLD_HASH}/out/libold-{OLD_HASH}.rlib"),
        format!("build/old/{OLD_HASH}/fingerprint/lib-old.json"),
        format!("build/app/{APP_HASH}/out/libapp-{APP_HASH}.rlib"),
    ] {
        assert!(exists(&repo, &format!("{DEBUG}/{kept}")), "{kept}");
    }
    assert!(!exists(
        &repo,
        &format!("{DEBUG}/build/serde/{SERDE_HASH}/out/serde-test")
    ));
    assert!(!exists(&repo, "target/doc") && !exists(&repo, &format!("{DEBUG}/app")));
    fs::metadata(repo.root().join(DEBUG)).unwrap_or_else(|e| unreachable!("{e}"));
}
