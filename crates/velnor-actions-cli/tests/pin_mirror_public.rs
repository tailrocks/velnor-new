// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, io, path::Path};

use serde_json::Value;
use velnor_actions_freshness::BaseRef;

#[path = "pin_mirror_public_fixture.rs"]
mod fixture;

use fixture::{
    CATALOG, Fixture, POLICY, TestResult, age_all_evidence, compiled_crane, compiled_tool_pins,
    set_first_line, set_policy_tool,
};

#[test]
fn pin_mirror_passes_shipped_pins_ignoring_evidence_age() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.edit_inventory(age_all_evidence)?;
    let outcome = fixture.run(None)?;
    assert!(outcome.success, "shipped pins rejected: {:?}", outcome.rows);
    assert!(outcome.stdout.contains("pin-mirror: PASS"));
    assert!(!outcome.stdout.contains("check-freshness"));
    for (name, version) in compiled_tool_pins() {
        let row = outcome
            .row("policy-mirror", "pass", &format!("tool {name}"))
            .ok_or_else(|| io::Error::other(format!("policy-mirror pass row missing: {name}")))?;
        assert_eq!(row["detail"], version.as_str(), "policy pin for {name}");
    }
    assert_eq!(outcome.rows_of("pin-diff", "info").len(), 0);
    Ok(())
}

#[test]
fn pin_mirror_rejects_drift_for_every_tool_the_old_gate_covered() -> TestResult {
    for (name, _) in compiled_tool_pins() {
        let fixture = Fixture::new()?;
        fixture.rewrite(POLICY, |text| set_policy_tool(&text, name, "999.0.0"))?;
        let outcome = fixture.run(None)?;
        assert!(!outcome.success, "policy drift accepted for {name}");
        assert!(outcome.stderr.contains("pin-mirror: FAIL"));
        assert!(!outcome.stderr.contains("check-freshness"));
        assert!(
            outcome
                .row("policy-mirror", "fail", &format!("tool {name}"))
                .is_some(),
            "policy drift for {name} not named: {:?}",
            outcome.rows
        );

        let fixture = Fixture::new()?;
        fixture.edit_inventory(|inventory| {
            for tool in inventory["tools"].as_array_mut().into_iter().flatten() {
                if tool["name"] == name {
                    tool["pinned"] = Value::String("999.0.0".to_owned());
                }
            }
        })?;
        let outcome = fixture.run(None)?;
        assert!(!outcome.success, "inventory drift accepted for {name}");
        assert!(
            outcome
                .row("local-pin", "fail", &format!("tool {name}"))
                .is_some(),
            "inventory drift for {name} not named: {:?}",
            outcome.rows
        );
    }
    Ok(())
}

#[test]
fn pin_mirror_rejects_action_and_runner_drift() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.rewrite(POLICY, |text| {
        set_first_line(
            &text,
            "sha = ",
            "sha = \"0000000000000000000000000000000000000000\"",
        )
    })?;
    let outcome = fixture.run(None)?;
    assert!(!outcome.success);
    assert!(outcome.row("policy-mirror", "fail", "action ").is_some());

    let fixture = Fixture::new()?;
    fixture.rewrite(POLICY, |text| {
        set_first_line(&text, "default = ", "default = \"ubuntu-0.0\"")
    })?;
    let outcome = fixture.run(None)?;
    assert!(!outcome.success);
    assert!(
        outcome
            .row("policy-mirror", "fail", "runner default")
            .is_some()
    );
    Ok(())
}

#[test]
fn pin_mirror_reports_pin_diff_without_changing_verdict() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.commit_all()?;

    let unchanged = fixture.run(Some("HEAD"))?;
    assert!(unchanged.success);
    let rows = unchanged.rows_of("pin-diff", "info");
    assert_eq!(rows.len(), 1, "unexpected rows: {rows:?}");
    assert!(
        rows[0]["detail"]
            .as_str()
            .is_some_and(|d| d.starts_with("unchanged vs"))
    );

    let old = compiled_crane();
    let bumped_version = "999.0.1";
    fixture.rewrite(POLICY, |text| {
        set_policy_tool(&text, "crane", bumped_version)
    })?;
    fixture.rewrite(CATALOG, |text| {
        text.replacen(
            &format!("CRANE_VERSION: &str = \"{old}\""),
            &format!("CRANE_VERSION: &str = \"{bumped_version}\""),
            1,
        )
    })?;
    fixture.edit_inventory(|inventory| {
        for tool in inventory["tools"].as_array_mut().into_iter().flatten() {
            if tool["name"] == "crane" {
                tool["pinned"] = Value::String(bumped_version.to_owned());
            }
        }
    })?;
    let bumped = fixture.run(Some("refs/heads/main"))?;
    assert!(
        bumped.success,
        "consistent bump rejected: {:?}",
        bumped.rows
    );
    let diff = bumped.rows_of("pin-diff", "info");
    assert_eq!(diff.len(), 2, "unexpected rows: {diff:?}");
    for subject in ["policy tool crane", "inventory tool crane"] {
        let row = bumped
            .row("pin-diff", "info", subject)
            .ok_or_else(|| io::Error::other(format!("pin-diff row missing: {subject}")))?;
        assert_eq!(
            row["detail"],
            format!("base={old} head={bumped_version}").as_str()
        );
    }

    fixture.rewrite(POLICY, |text| set_policy_tool(&text, "crane", "999.0.2"))?;
    let drifted = fixture.run(Some("HEAD"))?;
    assert!(!drifted.success, "drift accepted despite a valid base");
    Ok(())
}

#[test]
fn pin_mirror_marks_missing_base_files_absent_and_fails_closed_on_git_problems() -> TestResult {
    let fixture = Fixture::new()?;
    fs::rename(fixture.path(".velnor"), fixture.path("held-velnor"))?;
    fixture.commit_all()?;
    fs::rename(fixture.path("held-velnor"), fixture.path(".velnor"))?;
    let outcome = fixture.run(Some("HEAD"))?;
    assert!(
        outcome.success,
        "absent base files rejected: {:?}",
        outcome.rows
    );
    let row = outcome
        .row("pin-diff", "info", "policy tool crane")
        .ok_or("absent-base row missing")?;
    assert_eq!(
        row["detail"],
        format!("base=<absent> head={}", compiled_crane()).as_str()
    );

    let unresolved = fixture.run(Some("refs/heads/missing"))?;
    assert!(!unresolved.success);
    assert!(
        unresolved
            .row("pin-diff", "fail", "refs/heads/missing")
            .is_some()
    );

    let fixture = Fixture::new()?;
    let real = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(POLICY);
    fs::remove_file(fixture.path(POLICY))?;
    std::os::unix::fs::symlink("schema=1", fixture.path(POLICY))?;
    fixture.commit_all()?;
    fs::remove_file(fixture.path(POLICY))?;
    fs::copy(&real, fixture.path(POLICY))?;
    let linked = fixture.run(Some("HEAD"))?;
    assert!(!linked.success, "symlinked base policy accepted");
    assert!(linked.row("pin-diff", "fail", "HEAD").is_some());
    Ok(())
}

#[test]
fn base_ref_parse_rejects_unsafe_refs() -> TestResult {
    for accepted in ["origin/main", "HEAD", "refs/heads/a-b"] {
        BaseRef::parse(accepted).map_err(io::Error::other)?;
    }
    let long = "a".repeat(201);
    for rejected in [
        "",
        "-x",
        "a..b",
        "HEAD@{0}",
        "a b",
        "r\u{e9}f",
        long.as_str(),
    ] {
        assert!(BaseRef::parse(rejected).is_err(), "accepted {rejected:?}");
    }

    let fixture = Fixture::new()?;
    assert!(!fixture.run(Some("--output=x"))?.success);
    let empty = fixture.run(Some(""))?;
    assert!(empty.success);
    assert_eq!(empty.rows_of("pin-diff", "info").len(), 0);
    Ok(())
}
