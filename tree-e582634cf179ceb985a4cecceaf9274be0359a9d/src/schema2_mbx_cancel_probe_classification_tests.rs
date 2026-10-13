//! Execute the classifier against exact synthetic cache and run receipts.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use super::{fake_bin, run_bash, temp_dir};
use crate::schema2::mbx_cancel_probe::scripts;

const FIXTURE_KEY: &str = "linux-x64-mbx-velnor-mbx-1.22.0-dir-rust-1.98.1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-scope-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-run-123-attempt-1-cccccccccccccccccccccccccccccccccccccccc";

struct ClassifierFixture {
    root: PathBuf,
    bin: PathBuf,
    observer: PathBuf,
    envs: Vec<(String, String)>,
}

#[test]
fn classifier_requires_proven_windows_and_preserves_unknown_reservations()
-> Result<(), Box<dyn Error>> {
    assert_eq!(classify("pre-save", false, false)?, "MUSTMISS");
    assert_eq!(classify("during-save", true, true)?, "HIT");
    assert_eq!(
        classify("during-save", true, false)?,
        "RESERVATION-UNKNOWN/INCONCLUSIVE"
    );
    assert_eq!(classify("during-save", false, false)?, "NOT_RUN");
    Ok(())
}

#[test]
fn missing_restore_outputs_stay_not_run_after_a_proven_upload_window()
-> Result<(), Box<dyn Error>> {
    for missing in ["all", "primary", "conclusion", "failed"] {
        assert_restore_not_run(missing)?;
    }
    Ok(())
}

fn assert_restore_not_run(missing: &str) -> Result<(), Box<dyn Error>> {
    let mut fixture = setup_classifier("during-save", true, false)?;
    for (key, value) in &mut fixture.envs {
        if matches!(missing, "all" | "failed")
            && matches!(key.as_str(), "RESTORE_HIT" | "MATCHED_KEY")
        {
            value.clear();
        }
        if matches!(missing, "all" | "primary") && key == "RESTORE_PRIMARY_KEY" {
            value.clear();
        }
        if matches!(missing, "all" | "conclusion") && key == "RESTORE_CONCLUSION" {
            value.clear();
        }
        if missing == "failed" && key == "RESTORE_CONCLUSION" {
            value.clear();
            value.push_str("failure");
        }
    }
    let output = run_bash(
        scripts::OBSERVER_CLASSIFY,
        &fixture.root,
        &fixture.bin,
        &fixture.envs,
    )?;
    assert!(output.status.success());
    let result = fs::read_to_string(fixture.observer.join("result.json"))?;
    assert!(result.contains("\"outcome\":\"NOT_RUN\""), "{result}");
    assert!(
        result.contains("restore_action_outputs_missing_or_inconsistent"),
        "{result}"
    );
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}

fn classify(phase: &str, upload_started: bool, committed: bool) -> Result<String, Box<dyn Error>> {
    let fixture = setup_classifier(phase, upload_started, committed)?;
    let output = run_bash(
        scripts::OBSERVER_CLASSIFY,
        &fixture.root,
        &fixture.bin,
        &fixture.envs,
    )?;
    assert!(
        output.status.success(),
        "classifier failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = fs::read_to_string(fixture.observer.join("result.json"))?;
    let outcome = [
        "MUSTMISS",
        "HIT",
        "RESERVATION-UNKNOWN/INCONCLUSIVE",
        "NOT_RUN",
    ]
    .into_iter()
    .find(|candidate| result.contains(&format!("\"outcome\":\"{candidate}\"")))
    .ok_or_else(|| std::io::Error::other("classifier outcome missing"))?
    .to_owned();
    fs::remove_dir_all(fixture.root)?;
    Ok(outcome)
}

fn setup_classifier(
    phase: &str,
    upload_started: bool,
    committed: bool,
) -> Result<ClassifierFixture, Box<dyn Error>> {
    let root = temp_dir("classify")?;
    let bin = fake_bin(&root)?;
    let observer = root.join("mbx-cancel/observer");
    fs::create_dir_all(&observer)?;
    fs::write(
        observer.join("child-evidence.json"),
        child_evidence(phase, upload_started, committed),
    )?;
    fs::write(observer.join("import-count"), "7\n")?;
    fs::write(observer.join("reuse-count"), "2\n")?;
    let summary = root.join("summary.md");
    fs::write(&summary, "")?;
    let envs = classifier_env(&root, &summary, phase, committed);
    Ok(ClassifierFixture {
        root,
        bin,
        observer,
        envs,
    })
}

fn child_evidence(phase: &str, upload_started: bool, committed: bool) -> String {
    let save_started = phase == "during-save";
    let save_conclusion = if save_started { "cancelled" } else { "skipped" };
    let after_count = usize::from(committed);
    let after = if committed {
        format!(
            r#"{{"count":1,"caches":[{{"id":5,"key":"{FIXTURE_KEY}","ref":"refs/heads/main","size_in_bytes":1000,"last_accessed_at":"2026-10-04T00:00:00Z"}}]}}"#
        )
    } else {
        format!(r#"{{"count":{after_count},"caches":[]}}"#)
    };
    format!(
        r#"{{"save_step_started":{save_started},"save_step_conclusion":"{save_conclusion}","cancel_step_conclusion":"cancelled","upload_started_before_cancel":{upload_started},"restore_clean_miss":{},"cache_before":{{"count":0,"caches":[]}},"cache_after":{after}}}"#,
        !committed
    )
}

fn classifier_env(
    root: &std::path::Path,
    summary: &std::path::Path,
    phase: &str,
    committed: bool,
) -> Vec<(String, String)> {
    let (controller_mode, cache_scope) = if phase == "during-save" {
        (
            "mbx-cancel-during-save-controller",
            "qualification-mbx-v1/cancel-during-save-victim",
        )
    } else {
        (
            "mbx-cancel-pre-save-controller",
            "qualification-mbx-v1/cancel-pre-save-victim",
        )
    };
    let restore_hit = if committed { "true" } else { "" };
    let matched_key = if committed { FIXTURE_KEY } else { "" };
    vec![
        ("RUNNER_TEMP".to_owned(), root.display().to_string()),
        (
            "GITHUB_STEP_SUMMARY".to_owned(),
            summary.display().to_string(),
        ),
        ("PROBE_PHASE".to_owned(), phase.to_owned()),
        ("CONTROLLER_MODE".to_owned(), controller_mode.to_owned()),
        (
            "PROBE_ID".to_owned(),
            "0123456789abcdef0123456789abcdef".to_owned(),
        ),
        ("CACHE_SCOPE".to_owned(), cache_scope.to_owned()),
        ("MBX_VERSION".to_owned(), "1.22.0".to_owned()),
        ("GENERATION".to_owned(), "velnor-mbx-1.22.0".to_owned()),
        ("SHOULD_OBSERVE".to_owned(), "true".to_owned()),
        ("DERIVED_KEY".to_owned(), FIXTURE_KEY.to_owned()),
        ("VALIDATED_CACHE_KEY".to_owned(), FIXTURE_KEY.to_owned()),
        ("RESTORE_HIT".to_owned(), restore_hit.to_owned()),
        ("MATCHED_KEY".to_owned(), matched_key.to_owned()),
        ("RESTORE_PRIMARY_KEY".to_owned(), FIXTURE_KEY.to_owned()),
        ("RESTORE_CONCLUSION".to_owned(), "success".to_owned()),
        ("CONTROLLER_READY".to_owned(), "true".to_owned()),
        (
            "CONTROLLER_READY_REASON".to_owned(),
            "exact_identity_and_cancel_window_ready".to_owned(),
        ),
        ("CONTROLLER_CANCEL_REQUESTED".to_owned(), "true".to_owned()),
        ("CONTROLLER_CANCEL_STATUS".to_owned(), "202".to_owned()),
        ("CONTROLLER_POST_REVALIDATED".to_owned(), "true".to_owned()),
        (
            "CONTROLLER_CANCEL_AT".to_owned(),
            "2026-10-04T00:00:00Z".to_owned(),
        ),
        ("CONTROLLER_TERMINAL".to_owned(), "true".to_owned()),
        (
            "CONTROLLER_TERMINAL_STATE".to_owned(),
            "completed/cancelled".to_owned(),
        ),
        ("CONTROLLER_BEFORE_COUNT".to_owned(), "0".to_owned()),
    ]
}
