//! A skipped receipt gate still writes explicit NOT_RUN evidence.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use super::super::controller_fixtures::Fixture;
use super::{prepare_observer_root, run_bash};
use crate::schema2::mbx_cancel_probe::scripts;

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
        ("RESTORE_PRIMARY_KEY".to_owned(), String::new()),
        ("RESTORE_CONCLUSION".to_owned(), String::new()),
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
    prepare_observer_root(
        &fixture.root,
        &fixture.bin,
        &env.iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>(),
    )?;
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
    let evidence = fs::read_to_string(
        fixture
            .root
            .join("mbx-cancel-observer/observer/result.json"),
    )?;
    assert!(evidence.contains("\"outcome\":\"NOT_RUN\""), "{evidence}");
    assert!(evidence.contains("\"should_observe\":false"), "{evidence}");
    assert!(evidence.contains("\"cache_before_count\":-1"), "{evidence}");
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}
