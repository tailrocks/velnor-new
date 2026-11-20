// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use serde_json::json;
use std::fs;
use std::os::unix::fs::symlink;
use std::process::{Command, Output};
use velnor_ci::inputs::{snapshot_file, witness_file};
use velnor_ci::report::LaneMetrics;

type ResultTest = Result<(), Box<dyn std::error::Error>>;
fn cli(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("VELNOR_TESTS_RUN", "7")
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"))
}
fn read(repo: &Repo) -> LaneMetrics {
    serde_json::from_slice(
        &fs::read(repo.root().join("ci-metrics.json"))
            .unwrap_or_else(|error| unreachable!("{error}")),
    )
    .unwrap_or_else(|error| unreachable!("{error}"))
}

fn report(
    repo: &Repo,
    cache: &serde_json::Value,
    steps: &str,
) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("VELNOR_TESTS_RUN", "7")
        .env(
            "VELNOR_REPORT_CACHES",
            cache.to_string().replace('$', "\\u0024"),
        )
        .env("VELNOR_REPORT_STEPS", steps)
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(read(repo))
}

#[test]
fn report_preserves_restored_inputs_after_save_and_measures_failed_cargo_output() -> ResultTest {
    let repo = fixture("report-provenance")?;
    repo.git(&["add", "."]);
    assert!(cli(&repo, &["inputs", "save"]).status.success());
    assert!(cli(&repo, &["inputs", "restore"]).status.success());
    let before = fs::read(repo.root().join("target").join(snapshot_file("")))?;
    let witness = fs::read(repo.root().join("target").join(witness_file("")))?;
    repo.write("src/lib.rs", "pub fn changed() {}\n");
    assert!(cli(&repo, &["inputs", "save"]).status.success());
    assert_eq!(
        fs::read(repo.root().join("target").join(snapshot_file("")))?,
        before
    );
    assert_eq!(
        fs::read(repo.root().join("target").join(witness_file("")))?,
        witness
    );
    let fixture = repo.root().join("cargo-fixture");
    symlink("/bin/sh", &fixture)?;
    let script = interrupted_output(&repo)?;
    assert_eq!(
        cli(
            &repo,
            &["run", "--", fixture.to_str().ok_or("path")?, "-c", &script]
        )
        .status
        .code(),
        Some(7)
    );
    let cache = json!([{"family":"target","root":".","restore_step":"target_root","save_step":"save_target_root"}, {"family":"target","root":"literal${{root}}","restore_step":"absent","save_step":"absent"}]);
    let report = |steps: &str| report(&repo, &cache, steps);
    let steps = r#"{"target_root":{"outcome":"success","outputs":{"cache-primary-key":"primary-marker","cache-matched-key":"matched-marker"}}}"#;
    let metrics = report(steps)?;
    assert_eq!(metrics.caches[0].requested_key, "primary-marker");
    assert_eq!(metrics.caches[1].root.as_deref(), Some("literal${{root}}"));
    assert!(metrics.caches[0].usable_same_lock);
    assert!(
        !metrics.invocations[0].compilations.is_empty(),
        "{metrics:?}"
    );
    assert_eq!(metrics.invocations[0].compilations[0].package, "demo");
    assert!(metrics.invocations[0].compilations[0].workspace);
    assert!(!metrics.invocations[0].compilations[0].unchanged);
    assert!(metrics.violations.is_empty(), "{:?}", metrics.violations);
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|message| message.contains("dependency v1.0.0"))
    );
    assert_eq!(metrics.invocations[0].artifacts.len(), 3);
    assert_eq!(metrics.invocations[0].rejected, 1);
    assert!(
        metrics
            .diagnostics
            .iter()
            .any(|text| text.contains("identity unknown for 1"))
    );
    assert_eq!(metrics.invocations[0].compilations.len(), 1);
    assert_eq!(metrics.tests_run, Some(7));
    assert_eq!(
        metrics.slowest_tests,
        vec![("demo::it failing_test".to_owned(), 0.125)]
    );
    assert!(metrics.invocations[0].seconds > 0.0);
    repo.write(
        "Cargo.lock",
        "version = 4\n[[package]]\nname='demo'\nversion='0.2.0'\n",
    );
    let metrics = report(steps)?;
    assert!(!metrics.caches[0].usable_same_lock);
    assert!(metrics.violations.is_empty(), "{:?}", metrics.violations);
    fs::write(repo.root().join("velnor-run.log"), b"")?;
    let decision = cli(&repo, &["cache", "decide", "--target-dir", "target"]);
    assert!(decision.status.success(), "{decision:?}");
    assert!(String::from_utf8_lossy(&decision.stdout).contains("2 crates compiled"));
    let metrics = report(
        r#"{"target_root":{"outcome":"success","outputs":{"cache-matched-key":"matched-marker"}}}"#,
    )?;
    assert_eq!(metrics.caches[0].requested_key, "");
    Ok(())
}

#[test]
fn checker_rejects_missing_malformed_and_duplicate_artifacts_and_accepts_skipped_lanes()
-> ResultTest {
    let repo = Repo::new("report-check");
    let check = |needs: &str| {
        Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["report", "--check"])
            .arg(repo.root().join("artifacts"))
            .env("VELNOR_REPORT_NEEDS", needs)
            .output()
            .unwrap_or_else(|error| unreachable!("{error}"))
    };
    assert!(check("{}").status.success());
    for needs in ["[]", "null", r#""rust""#, "1", "false"] {
        let output = check(needs);
        assert!(!output.status.success(), "{needs}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("invalid type"),
            "{needs}: {output:?}"
        );
    }
    assert!(check(r#"{"rust":{"result":"skipped"}}"#).status.success());
    assert!(
        check(r#"{"rust-trim":{"result":"skipped"}}"#)
            .status
            .success()
    );
    assert!(
        !check(r#"{"rust-trim":{"result":"failure"}}"#)
            .status
            .success()
    );
    assert!(
        !check(r#"{"rust-trim":{"result":"cancelled"}}"#)
            .status
            .success()
    );
    assert!(!check(r#"{"rust":{"result":"failure"}}"#).status.success());
    fs::create_dir_all(repo.root().join("artifacts/rust"))?;
    let mut value = crate::report::lane();
    value["caches"] = json!([]);
    fs::write(
        repo.root().join("artifacts/rust/ci-metrics.json"),
        value.to_string(),
    )?;
    assert!(check(r#"{"rust":{"result":"success"}}"#).status.success());
    fs::write(
        repo.root().join("artifacts/rust/ci-metrics.json"),
        "malformed",
    )?;
    assert!(!check(r#"{"rust":{"result":"success"}}"#).status.success());
    Ok(())
}

#[test]
fn failed_setup_still_writes_metrics_with_unknown_provenance() {
    let repo = Repo::new("report-failed-setup");
    assert!(cli(&repo, &["report"]).status.success());
    let metrics = read(&repo);
    assert_eq!(metrics.first_cargo_seconds, None);
    assert_eq!(metrics.violations, [] as [String; 0]);
}

#[test]
fn local_report_does_not_contact_github_even_with_an_ambient_token() -> ResultTest {
    let repo = Repo::new("report-local-no-api");
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = finished.clone();
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut contacted = false;
        while !stopped.load(std::sync::atomic::Ordering::SeqCst) {
            if let Ok((mut stream, _)) = listener.accept() {
                contacted = true;
                stream.set_nonblocking(false).expect("blocking API socket");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(1)))
                    .expect("bounded API request");
                let mut request = [0; 4096];
                assert!(
                    stream
                        .read(&mut request)
                        .unwrap_or_else(|error| unreachable!("{error}"))
                        > 0
                );
                stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap_or_else(|error| unreachable!("{error}"));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        contacted
    });
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("GITHUB_REPOSITORY", "owner/repo")
        .env("GITHUB_RUN_ID", "1")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_TOKEN", "fixture")
        .env("GITHUB_API_URL", format!("http://{address}"))
        .output();
    finished.store(true, std::sync::atomic::Ordering::SeqCst);
    let contacted = server.join().map_err(|_| "API listener failed")?;
    assert!(output?.status.success());
    assert!(!contacted, "local report contacted GitHub");
    assert_eq!(read(&repo).first_cargo_seconds, None);
    Ok(())
}

#[test]
fn report_accepts_only_cache_plans_and_observes_steps_without_cargo() -> ResultTest {
    let repo = Repo::new("report-cache-plan");
    let mut plan =
        json!([{"family":"target","root":".","restore_step":"restore","save_step":"save"}]);
    let metrics = report(
        &repo,
        &plan,
        r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#,
    )?;
    assert_eq!(metrics.caches[0].hit, velnor_ci::report::Hit::Exact);
    assert_eq!(metrics.caches[0].requested_key, "key");
    assert!(!metrics.caches[0].usable);
    for (field, value) in [("usable", json!(true)), ("hit", json!("exact"))] {
        plan[0][field] = value;
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .arg("report")
            .current_dir(repo.root())
            .env("RUNNER_TEMP", repo.root())
            .env("VELNOR_REPORT_CACHES", plan.to_string())
            .env_remove("GITHUB_TOKEN")
            .env_remove("CARGO_TARGET_DIR")
            .output()?;
        assert!(!output.status.success(), "{field}: {output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains(field));
        plan[0].as_object_mut().ok_or("plan object")?.remove(field);
    }
    Ok(())
}

fn interrupted_output(repo: &Repo) -> Result<String, Box<dyn std::error::Error>> {
    let root = fs::canonicalize(repo.root())?;
    let artifact = json!({
        "reason":"compiler-artifact", "package_id":format!("path+file://{}#demo@0.1.0", root.display()), "manifest_path":root.join("Cargo.toml"),
        "target":{"name":"demo","kind":["lib"],"crate_types":["lib"],"src_path":root.join("src/lib.rs")},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":["default"], "filenames":["target/debug/deps/libdemo-opaque.rmeta"], "fresh":false
    }).to_string();
    let script = format!(
        "printf '   Compiling demo v0.1.0 ({})\\n   Compiling dependency v1.0.0\\n        FAIL [   0.125s] demo::it failing_test\\n     Summary [   0.125s] 2 tests run: 1 passed, 1 failed\\n'; true",
        fs::canonicalize(repo.root())?.display()
    );
    let script = format!(
        r#"printf '%s' '{}'; while ! grep -Fq '{}' velnor-run.log; do sleep 0.01; done; {{ {script}; }} >&2; while ! grep -Fq '2 tests run' velnor-run.log; do sleep 0.01; done; printf '\377\n' >&2; printf '%s\n' '{}'; printf '%s\n' '{}'; printf '%s\n' '{}'; printf '%s\n' '{{"reason":"compiler-artifact"}}'; exit 7"#,
        &artifact[..10],
        &artifact[..10],
        &artifact[10..],
        artifact.replace(".rmeta", ".rlib"),
        artifact.replace(
            "target/debug/deps/libdemo-opaque.rmeta",
            "target/debug/libdemo.dylib"
        )
    );
    Ok(script)
}

fn metadata_inputs(repo: &Repo) -> ResultTest {
    let samples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../generator/samples");
    for (from, to) in [
        ("full-run-cron/view/mise.toml", "mise.toml"),
        ("full-run-cron/view/mise.lock", "mise.lock"),
        (
            "helper-lock/view/.velnor/rust-ci.lock",
            ".velnor/rust-ci.lock",
        ),
    ] {
        repo.write(to, &fs::read_to_string(samples.join(from))?);
    }
    Ok(())
}

mod api;
mod build_failure;
mod evidence;
mod policy;
mod provenance;
mod target_identity;
mod waste;
mod wrapped;

pub(crate) fn fixture(name: &str) -> Result<Repo, Box<dyn std::error::Error>> {
    let repo = Repo::new(name);
    repo.write_all(&[
        (
            "Cargo.toml",
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("src/lib.rs", "pub fn value() {}\n"),
        (
            "Cargo.lock",
            "version = 4\n[[package]]\nname='demo'\nversion='0.1.0'\n",
        ),
    ]);
    metadata_inputs(&repo)?;
    velnor_gen::io::write_metadata(repo.root())?;
    Ok(repo)
}
