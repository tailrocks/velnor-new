//! Malformed cache API bodies never become evidence of an empty namespace.

use std::error::Error;
use std::fs;

use super::controller_fixtures::{Fixture, expected_key};
use super::{run_bash, scripts};

const INVALID_SHAPES: [&str; 7] = [
    "cache-object",
    "cache-null-response",
    "cache-null-entry",
    "cache-missing-array",
    "cache-missing-total",
    "cache-count-mismatch",
    "cache-truncated-page",
];

#[test]
fn malformed_cache_shapes_remain_unknown_on_all_snapshot_paths()
-> Result<(), Box<dyn Error>> {
    for mode in INVALID_SHAPES {
        assert_controller_cache_unknown(mode)?;
        assert_observer_before_unknown(mode)?;
        assert_observer_after_unknown(mode)?;
    }
    Ok(())
}

#[test]
fn valid_cache_arrays_keep_exact_zero_and_hit_counts() -> Result<(), Box<dyn Error>> {
    assert_observer_before_count("good", 0)?;
    assert_observer_before_count("cache-valid-record", 1)?;
    Ok(())
}

fn assert_controller_cache_unknown(mode: &str) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(&format!("controller-cache-{mode}"), false)?;
    fixture.dispatch("good")?;
    fixture.readiness("good")?;
    let cancel = fixture.cancel(mode)?;
    assert!(cancel.contains("cancel_requested=true\n"), "{cancel}");
    let snapshot =
        fs::read_to_string(fixture.root.join("mbx-cancel-controller/cache-before-exact.json"))?;
    assert!(snapshot.contains("\"count\":-1"), "{mode}: {snapshot}");
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}

fn assert_observer_before_unknown(mode: &str) -> Result<(), Box<dyn Error>> {
    assert_observer_before(mode, -1)
}

fn assert_observer_before_count(mode: &str, count: i64) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(&format!("observer-before-{mode}"), false)?;
    let output = fixture.output("cache-before");
    fs::write(&output, "")?;
    let mut env = fixture.env(&output, mode);
    env.push(("VALIDATED_CACHE_KEY".to_owned(), expected_key()));
    let script = scripts::observer_cache_before();
    let result = run_bash(&script, &fixture.root, &fixture.bin, &env)?;
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let snapshot = fs::read_to_string(fixture.root.join("mbx-cancel/observer/cache-before.json"))?;
    assert!(snapshot.contains(&format!("\"count\":{count}")), "{mode}: {snapshot}");
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}

fn assert_observer_before(mode: &str, expected_count: i64) -> Result<(), Box<dyn Error>> {
    assert_observer_before_count(mode, expected_count)
}

fn assert_observer_after_unknown(mode: &str) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(&format!("observer-after-{mode}"), false)?;
    let output = fixture.output("evidence");
    fs::write(&output, "")?;
    let mut env = fixture.env(&output, mode);
    env.extend([
        ("VALIDATED_CACHE_KEY".to_owned(), expected_key()),
        (
            "CHILD_SOURCE_SHA".to_owned(),
            super::controller_fixtures::SOURCE_SHA.to_owned(),
        ),
        ("CHILD_ACTOR".to_owned(), "github-actions[bot]".to_owned()),
        ("CURL_LOCATION_MODE".to_owned(), "missing".to_owned()),
        ("CONTROLLER_CANCEL_REQUESTED".to_owned(), "true".to_owned()),
        ("CONTROLLER_CANCEL_AT".to_owned(), "2026-10-04T00:00:10Z".to_owned()),
        ("CONTROLLER_BEFORE_COUNT".to_owned(), "0".to_owned()),
    ]);
    let script = scripts::observer_evidence();
    let result = run_bash(&script, &fixture.root, &fixture.bin, &env)?;
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let evidence = fs::read_to_string(fixture.root.join("mbx-cancel/observer/child-evidence.json"))?;
    assert!(
        evidence.contains("\"cache_after\":{\"count\":-1"),
        "{mode}: {evidence}"
    );
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}
