//! Save-step log evidence must be present and predate the cancel request.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use super::controller_fixtures::{Fixture, expected_key};
use super::run_bash;
use crate::schema2::mbx_cancel_probe::scripts;

struct EvidenceRun {
    started: bool,
    env: BTreeMap<String, String>,
    observer: PathBuf,
    curl_log: PathBuf,
}

#[test]
fn missing_or_untrusted_save_log_evidence_stays_not_run() -> Result<(), Box<dyn Error>> {
    for mode in [
        "missing",
        "malformed",
        "large-header",
        "large-body",
        "signed-http-redirect",
        "no-marker",
        "valid-progress",
        "late-progress",
        "zero-progress",
        "complete-progress",
        "malformed-progress",
    ] {
        assert_log_case(mode)?;
    }
    Ok(())
}

#[test]
fn skipped_receipt_validation_still_writes_not_run_result() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("skipped-receipt-classify", false)?;
    let summary = fixture.root.join("summary.md");
    fs::write(&summary, "")?;
    let output = fixture.output("classify-missing-receipt");
    fs::write(&output, "")?;
    let mut env = BTreeMap::from_iter(fixture.env(&output, "missing-receipt"));
    env.extend([
        ("SHOULD_OBSERVE".to_owned(), String::new()),
        ("CONTROLLER_BEFORE_COUNT".to_owned(), String::new()),
        ("DERIVED_KEY".to_owned(), String::new()),
        ("VALIDATED_CACHE_KEY".to_owned(), String::new()),
        ("RESTORE_HIT".to_owned(), String::new()),
        ("MATCHED_KEY".to_owned(), String::new()),
        ("CONTROLLER_READY".to_owned(), String::new()),
        ("CONTROLLER_READY_REASON".to_owned(), String::new()),
        ("CONTROLLER_CANCEL_REQUESTED".to_owned(), String::new()),
        ("CONTROLLER_CANCEL_STATUS".to_owned(), String::new()),
        ("CONTROLLER_POST_REVALIDATED".to_owned(), String::new()),
        ("CONTROLLER_TERMINAL".to_owned(), String::new()),
        ("CONTROLLER_TERMINAL_STATE".to_owned(), String::new()),
        ("CONTROLLER_CANCEL_AT".to_owned(), String::new()),
        ("GENERATION".to_owned(), "velnor-mbx-1.22.0".to_owned()),
        (
            "GITHUB_STEP_SUMMARY".to_owned(),
            summary.display().to_string(),
        ),
    ]);
    let result = run_bash(
        scripts::OBSERVER_CLASSIFY,
        &fixture.root,
        &fixture.bin,
        &env.into_iter().collect::<Vec<_>>(),
    )?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let evidence = fs::read_to_string(fixture.root.join("mbx-cancel/observer/result.json"))?;
    assert!(evidence.contains("\"outcome\":\"NOT_RUN\""), "{evidence}");
    assert!(evidence.contains("\"should_observe\":false"), "{evidence}");
    assert!(evidence.contains("\"cache_before_count\":-1"), "{evidence}");
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}

fn assert_log_case(mode: &str) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(&format!("log-{mode}"), false)?;
    let key = expected_key();
    let evidence = run_evidence(&fixture, mode, &key)?;
    assert_classification(
        &fixture,
        evidence.env,
        &evidence.observer,
        &key,
        evidence.started,
    )?;
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}

fn run_evidence(fixture: &Fixture, mode: &str, key: &str) -> Result<EvidenceRun, Box<dyn Error>> {
    let setup = prepare_evidence(fixture, mode, key)?;
    let output = run_bash(
        scripts::OBSERVER_EVIDENCE,
        &fixture.root,
        &fixture.bin,
        &setup
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>(),
    )?;
    assert!(
        output.status.success(),
        "{}: {}",
        mode,
        String::from_utf8_lossy(&output.stderr)
    );
    validate_evidence(mode, key, &setup)?;
    Ok(setup)
}

fn prepare_evidence(
    fixture: &Fixture,
    mode: &str,
    key: &str,
) -> Result<EvidenceRun, Box<dyn Error>> {
    fixture.install_curl()?;
    fixture.install_date()?;
    let observer = fixture.root.join("mbx-cancel/observer");
    fs::create_dir_all(&observer)?;
    fs::write(
        observer.join("cache-before.json"),
        "{\"count\":0,\"caches\":[]}\n",
    )?;
    let summary = fixture.root.join("summary.md");
    fs::write(&summary, "")?;
    let curl_log = fixture.root.join("curl.log");
    let mut env = BTreeMap::from_iter(fixture.env(&fixture.output("evidence"), "observer-window"));
    let marker = progress_marker(mode);
    env.extend([
        ("GH_TOKEN".to_owned(), "fixture-secret-token".to_owned()),
        ("VALIDATED_CACHE_KEY".to_owned(), key.to_owned()),
        ("CURL_LOG".to_owned(), curl_log.display().to_string()),
        ("CURL_LOCATION_MODE".to_owned(), mode.to_owned()),
        ("CURL_MARKER".to_owned(), marker.to_owned()),
        (
            "CHILD_SOURCE_SHA".to_owned(),
            super::controller_fixtures::SOURCE_SHA.to_owned(),
        ),
        ("CHILD_ACTOR".to_owned(), "github-actions[bot]".to_owned()),
        ("CONTROLLER_CANCEL_REQUESTED".to_owned(), "true".to_owned()),
        (
            "CONTROLLER_CANCEL_AT".to_owned(),
            "2026-10-04T00:00:10Z".to_owned(),
        ),
        ("CONTROLLER_BEFORE_COUNT".to_owned(), "0".to_owned()),
        (
            "GITHUB_STEP_SUMMARY".to_owned(),
            summary.display().to_string(),
        ),
    ]);
    let evidence_path = fixture.output("evidence");
    fs::write(&evidence_path, "")?;
    env.insert(
        "GITHUB_OUTPUT".to_owned(),
        evidence_path.display().to_string(),
    );
    Ok(EvidenceRun {
        started: mode == "valid-progress",
        env,
        observer,
        curl_log,
    })
}

fn progress_marker(mode: &str) -> &'static str {
    match mode {
        "valid-progress" => "partial",
        "late-progress" => "late",
        "zero-progress" => "zero",
        "complete-progress" => "complete",
        "malformed-progress" => "malformed",
        _ => "false",
    }
}

fn validate_evidence(mode: &str, key: &str, setup: &EvidenceRun) -> Result<(), Box<dyn Error>> {
    let evidence = fs::read_to_string(setup.observer.join("child-evidence.json"))?;
    let requests = if setup.curl_log.exists() {
        fs::read_to_string(&setup.curl_log)?
    } else {
        String::new()
    };
    assert!(
        evidence.contains(&format!(
            "\"upload_started_before_cancel\":{}",
            setup.started
        )),
        "{evidence}; requests={requests}"
    );
    if setup.started {
        assert!(
            evidence.contains("timestamped_positive_partial_progress_before_cancel"),
            "{evidence}"
        );
    }
    assert!(key.contains("velnor-mbx-1.22.0-dir-rust-1.98.1"));
    assert!(!evidence.contains("signed.example"));
    assert!(!evidence.contains("fixture-secret-token"));
    assert!(!setup.observer.join("save-log-response.txt").exists());
    assert!(!setup.observer.join("save-step.log").exists());
    assert!(!setup.observer.join("save-log-download.headers").exists());
    assert!(!setup.observer.join("save-log.curlrc").exists());
    assert!(!setup.observer.join("save-log-api-status").exists());
    assert_log_requests(&setup.curl_log, mode)?;
    Ok(())
}

fn assert_log_requests(path: &Path, mode: &str) -> Result<(), Box<dyn Error>> {
    let requests = fs::read_to_string(path)?;
    assert_eq!(
        requests.lines().next(),
        Some(
            "observer-api https://api.github.com/repos/tailrocks/velnor-new/actions/jobs/456/steps/2/logs true"
        )
    );
    if mode != "missing" && mode != "malformed" && mode != "large-header" {
        assert_eq!(
            requests.lines().nth(1),
            Some("observer-signed authorized=false")
        );
    } else {
        assert_eq!(requests.lines().count(), 1);
    }
    Ok(())
}

fn assert_classification(
    fixture: &Fixture,
    mut env: BTreeMap<String, String>,
    observer: &Path,
    key: &str,
    started: bool,
) -> Result<(), Box<dyn Error>> {
    env.extend([
        ("SHOULD_OBSERVE".to_owned(), "true".to_owned()),
        ("DERIVED_KEY".to_owned(), key.to_owned()),
        ("VALIDATED_CACHE_KEY".to_owned(), key.to_owned()),
        ("RESTORE_HIT".to_owned(), "false".to_owned()),
        ("MATCHED_KEY".to_owned(), String::new()),
        ("CONTROLLER_READY".to_owned(), "true".to_owned()),
        (
            "CONTROLLER_READY_REASON".to_owned(),
            "exact_identity_and_cancel_window_ready".to_owned(),
        ),
        ("CONTROLLER_CANCEL_STATUS".to_owned(), "202".to_owned()),
        ("CONTROLLER_POST_REVALIDATED".to_owned(), "true".to_owned()),
        ("CONTROLLER_TERMINAL".to_owned(), "true".to_owned()),
        (
            "CONTROLLER_TERMINAL_STATE".to_owned(),
            "completed/cancelled".to_owned(),
        ),
        ("GENERATION".to_owned(), "velnor-mbx-1.22.0".to_owned()),
    ]);
    let output = fixture.output("classify");
    fs::write(&output, "")?;
    env.insert("GITHUB_OUTPUT".to_owned(), output.display().to_string());
    let classified = run_bash(
        scripts::OBSERVER_CLASSIFY,
        &fixture.root,
        &fixture.bin,
        &env.into_iter().collect::<Vec<_>>(),
    )?;
    assert!(classified.status.success());
    let result = fs::read_to_string(observer.join("result.json"))?;
    if started {
        assert!(
            result.contains("\"outcome\":\"RESERVATION-UNKNOWN/INCONCLUSIVE\""),
            "{result}"
        );
    } else {
        assert!(result.contains("\"outcome\":\"NOT_RUN\""), "{result}");
    }
    Ok(())
}
