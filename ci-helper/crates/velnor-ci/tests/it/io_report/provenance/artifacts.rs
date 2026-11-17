// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn quiet_artifacts_count_once_and_only_nonfresh_units_are_waste()
-> Result<(), Box<dyn std::error::Error>> {
    let base = fixture("report-quiet-artifacts-base")?;
    restore(&base);
    quiet_nonfresh_artifact(&base)?;
    fresh_artifact_is_clean(&base)?;
    progress_and_artifact_count_once(&base)?;
    package_identity_is_preserved(&base)
}

fn quiet_nonfresh_artifact(base: &Repo) -> Result<(), Box<dyn std::error::Error>> {
    let quiet = base.fork("report-quiet-artifact");
    let lib = artifact(
        &quiet,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        false,
        false,
    )?;
    let metrics = observation(&quiet, &[], &[lib])?;
    assert_eq!(metrics.invocations[0].compilations.len(), 1, "{metrics:?}");
    assert!(
        metrics
            .violations
            .iter()
            .any(|line| line.contains("demo v0.1.0")),
        "{metrics:?}"
    );
    Ok(())
}

fn fresh_artifact_is_clean(base: &Repo) -> Result<(), Box<dyn std::error::Error>> {
    let fresh = base.fork("report-fresh-artifact");
    let lib = artifact(
        &fresh,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        true,
        false,
    )?;
    let metrics = observation(&fresh, &[], &[lib])?;
    assert!(metrics.invocations[0].compilations.is_empty());
    assert_eq!(metrics.violations, [] as [String; 0]);
    Ok(())
}

fn progress_and_artifact_count_once(base: &Repo) -> Result<(), Box<dyn std::error::Error>> {
    let progress_and_artifact = base.fork("report-progress-artifact");
    let lib = artifact(
        &progress_and_artifact,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        false,
        false,
    )?;
    let event = event(&progress_and_artifact, "Compiling", "demo", "0.1.0", true);
    let metrics = observation(&progress_and_artifact, &[event], &[lib])?;
    assert_eq!(metrics.invocations[0].compilations.len(), 1, "{metrics:?}");
    Ok(())
}

fn package_identity_is_preserved(base: &Repo) -> Result<(), Box<dyn std::error::Error>> {
    let both = base.fork("report-lib-test-artifacts");
    let artifacts = package_identity_artifacts(&both)?;
    fs::remove_file(both.root().join(SNAPSHOT))?;
    let metrics = observation(&both, &[], &artifacts)?;
    assert_eq!(metrics.invocations[0].compilations.len(), 3, "{metrics:?}");
    assert_eq!(
        metrics.invocations[0]
            .compilations
            .iter()
            .filter(|event| event.version == "0.1.0")
            .count(),
        2
    );
    assert_eq!(
        metrics.invocations[0]
            .compilations
            .iter()
            .filter(|event| event.version == "0.2.0")
            .count(),
        1
    );
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|error| { error.contains("non-fresh artifact package identity is unsupported") })
    );
    assert!(velnor_ci::report::check(&[metrics]).is_err());
    Ok(())
}

fn package_identity_artifacts(repo: &Repo) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let lib = artifact(
        repo,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        false,
        false,
    )?;
    let test = artifact(
        repo,
        "demo",
        "test",
        "target/debug/deps/demo-test-hash",
        false,
        true,
    )?;
    let mut other_source = artifact(
        repo,
        "demo",
        "lib",
        "target/debug/deps/libdemo-other-source.rmeta",
        false,
        false,
    )?;
    other_source["package_id"] = json!("git+https://example.invalid/demo?rev=deadbeef#demo@0.1.0");
    let mut other_version = artifact(
        repo,
        "demo",
        "lib",
        "target/debug/deps/libdemo-other-version.rmeta",
        false,
        false,
    )?;
    other_version["package_id"] = json!(format!(
        "path+file://{}#demo@0.2.0",
        fs::canonicalize(repo.root())?.display()
    ));
    let mut unsupported = artifact(
        repo,
        "demo",
        "lib",
        "target/debug/deps/libdemo-unsupported.rmeta",
        false,
        false,
    )?;
    unsupported["package_id"] = json!("not-a-package-id");
    Ok(vec![lib, test, other_source, other_version, unsupported])
}

#[test]
fn exact_source_downloads_require_a_changed_crate_archive() -> Result<(), Box<dyn std::error::Error>>
{
    let base = fixture("report-download-archive-base")?;
    let repo = base.fork("report-download-evidence");
    let source_cache =
        json!([{"family":"sources","root":".","restore_step":"restore","save_step":"save"}]);
    let steps = r#"{"restore":{"outputs":{"cache-primary-key":"source-key","cache-matched-key":"source-key","cache-hit":"true"}}}"#;

    let home = cargo_home(&repo)?;
    run_downloaded_cargo(&repo, &home, true)?;
    let metrics = super::report(&repo, &source_cache, steps)?;
    assert_eq!(
        metrics.invocations[0].verified_download_archives,
        Some(vec!["index/demo-0.1.0.crate".to_owned()])
    );
    assert_eq!(
        metrics.invocations[0].downloads,
        ["demo v0.1.0 (index/demo-0.1.0.crate)"]
    );
    assert!(
        metrics
            .diagnostics
            .iter()
            .any(|line| { line.contains("unverified download event for lookalike v9.9.9") })
    );
    assert!(
        metrics
            .violations
            .iter()
            .any(|line| line.contains("exact sources hit"))
    );

    let invocations = repo.root().join(INVOCATIONS);
    let record: Value = serde_json::from_slice(&fs::read(&invocations)?)?;
    let mut record = record;
    record["verified_download_archives"] = Value::Null;
    fs::write(&invocations, format!("{record}\n"))?;
    let metrics = super::report(&repo, &source_cache, steps)?;
    assert_eq!(metrics.invocations[0].verified_download_archives, None);
    assert_eq!(metrics.invocations[0].downloads, [] as [String; 0]);
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("crate archive evidence is unavailable"))
    );
    assert!(
        !metrics
            .violations
            .iter()
            .any(|line| line.contains("downloaded"))
    );
    Ok(())
}

#[test]
fn same_package_archives_in_two_registries_count_as_two_downloads() -> super::super::ResultTest {
    archive_downloads(
        &["other/demo-0.1.0.crate", "registry/demo-0.1.0.crate"],
        &[
            "demo v0.1.0 (other/demo-0.1.0.crate)",
            "demo v0.1.0 (registry/demo-0.1.0.crate)",
        ],
    )
}

#[test]
fn repeated_download_text_counts_one_changed_archive_once() -> super::super::ResultTest {
    archive_downloads(
        &["registry/demo-0.1.0.crate"],
        &["demo v0.1.0 (registry/demo-0.1.0.crate)"],
    )
}

#[test]
fn unrelated_archive_does_not_corroborate_download_text() -> super::super::ResultTest {
    archive_downloads(&["registry/unrelated-0.1.0.crate"], &[])
}

fn archive_downloads(archives: &[&str], expected: &[&str]) -> super::super::ResultTest {
    let repo = fixture("report-qualified-archive-downloads")?;
    let root = fs::canonicalize(repo.root())?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["root"] = json!(root);
    invocation["compilations"] = json!([]);
    invocation["artifacts"] = json!([]);
    invocation["progress"] = json!([
        {"kind":"Downloaded","package":"demo","version":"0.1.0","location":null},
        {"kind":"Downloaded","package":"demo","version":"0.1.0","location":null},
        {"kind":"Downloaded","package":"lookalike","version":"9.9.9","location":null}
    ]);
    invocation["verified_download_archives"] = json!(archives);
    fs::write(repo.root().join(INVOCATIONS), format!("{invocation}\n"))?;
    let caches =
        json!([{"family":"sources","root":".","restore_step":"restore","save_step":"save"}]);
    let metrics = super::super::report(
        &repo,
        &caches,
        r#"{"restore":{"outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#,
    )?;
    assert_eq!(metrics.invocations[0].downloads, expected);
    assert_eq!(metrics.downloaded, expected.len() as u64);
    assert_eq!(metrics.violations.is_empty(), expected.is_empty());
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    assert!(
        metrics
            .diagnostics
            .iter()
            .any(|line| line.contains("unverified download event for lookalike v9.9.9"))
    );
    Ok(())
}

fn cargo_home(repo: &Repo) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let home = repo.root().join("cargo-home");
    fs::create_dir_all(&home)?;
    Ok(home)
}

fn run_downloaded_cargo(
    repo: &Repo,
    cargo_home: &std::path::Path,
    create_archive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let archive = cargo_home
        .join("registry/cache/index")
        .join("demo-0.1.0.crate");
    let script = repo.root().join("cargo-fixture");
    let create = if create_archive {
        format!(
            "mkdir -p '{}'\nprintf archive > '{}'\n",
            archive.parent().ok_or("archive parent")?.display(),
            archive.display()
        )
    } else {
        String::new()
    };
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '    Downloaded demo v0.1.0\\n    Downloaded lookalike v9.9.9\\n' >&2\n{create}"
        ),
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    repo.git(&["add", "."]);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--"])
        .arg(&script)
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("CARGO_HOME", cargo_home)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(())
}
