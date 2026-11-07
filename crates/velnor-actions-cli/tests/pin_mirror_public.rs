// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, io, path::Path};

use serde_json::Value;
use velnor_actions_freshness::BaseRef;
use velnor_actions_mise::{ToolCatalog, catalog::lock::verify_version_policy};

#[path = "pin_mirror_public_fixture.rs"]
mod fixture;

#[path = "pin_mirror_public_sources.rs"]
mod sources;

use fixture::{
    CATALOG, Fixture, INVENTORY, POLICY, RUNNER_SOURCE, TestResult, age_all_evidence,
    compiled_crane, compiled_tool_pins, set_first_line, set_policy_tool,
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

#[test]
fn pin_mirror_and_candidate_agree_on_which_policy_files_are_valid() -> TestResult {
    let gh = compiled_tool_pins()
        .into_iter()
        .find_map(|(name, version)| (name == "gh").then_some(version))
        .ok_or("gh pin missing")?;
    let cases = [
        ("shipped", format!("gh = \"{gh}\""), true),
        ("quoted key", format!("\"gh\" = \"{gh}\""), true),
        ("literal string", format!("gh = '{gh}'"), true),
        ("drifted value", "gh = \"999.0.0\"".to_owned(), false),
        ("malformed", "gh = = broken".to_owned(), false),
    ];
    for (label, line, valid) in cases {
        let fixture = Fixture::new()?;
        let text = set_first_line(&fs::read_to_string(fixture.path(POLICY))?, "gh = ", &line);
        fs::write(fixture.path(POLICY), &text)?;
        let mirror = fixture.run(None)?.success;
        let candidate = verify_version_policy(&text, &ToolCatalog::pinned()).is_ok();
        assert_eq!(mirror, valid, "pin-mirror verdict for {label}");
        assert_eq!(candidate, valid, "candidate verdict for {label}");
    }
    Ok(())
}

#[test]
fn pin_diff_reports_cosign_digest_and_runner_label_changes() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.commit_all()?;
    let inventory: Value = serde_json::from_slice(&fs::read(fixture.path(INVENTORY))?)?;
    let cosign = inventory["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|tool| tool["name"] == "cosign")
        .ok_or("cosign inventory row missing")?;
    let old_digest = cosign["artifacts"]["linux-amd64"]["sha256"]
        .as_str()
        .ok_or("cosign linux-amd64 sha256 missing")?
        .to_owned();
    let new_digest = "ab".repeat(32);
    fixture.rewrite(CATALOG, |text| text.replacen(&old_digest, &new_digest, 1))?;
    fixture.edit_inventory(|inventory| {
        for tool in inventory["tools"].as_array_mut().into_iter().flatten() {
            if tool["name"] == "cosign" {
                tool["artifacts"]["linux-amd64"]["sha256"] = Value::String(new_digest.clone());
            }
        }
    })?;
    let digest = fixture.run(Some("HEAD"))?;
    assert!(digest.success, "consistent digest change rejected");
    let row = digest
        .row(
            "pin-diff",
            "info",
            "inventory tool cosign linux-amd64 sha256",
        )
        .ok_or_else(|| io::Error::other(format!("digest change unreported: {:?}", digest.rows)))?;
    assert_eq!(
        row["detail"],
        format!("base={old_digest} head={new_digest}").as_str()
    );

    let fixture = Fixture::new()?;
    fixture.commit_all()?;
    fixture.rewrite(POLICY, |text| {
        let changed = text.replacen(
            "supported = [\"ubuntu-26.04\"]",
            "supported = [\"ubuntu-26.04\", \"ubuntu-28.04\"]",
            1,
        );
        assert_ne!(changed, text);
        changed
    })?;
    fixture.edit_inventory(|inventory| {
        assert!(inventory["runner"]["supported"].is_array());
        if let Some(labels) = inventory["runner"]["supported"].as_array_mut() {
            assert!(!labels.iter().any(|label| label == "ubuntu-28.04"));
            labels.push(Value::String("ubuntu-28.04".to_owned()));
        }
    })?;
    fixture.rewrite(RUNNER_SOURCE, |text| {
        let changed = text.replacen(
            "[&str; 2] = [\"ubuntu-26.04\", \"ubuntu-26.04-arm\"]",
            "[&str; 3] = [\"ubuntu-26.04\", \"ubuntu-26.04-arm\", \"ubuntu-28.04\"]",
            1,
        );
        assert_ne!(changed, text);
        changed
    })?;
    let labels = fixture.run(Some("HEAD"))?;
    assert!(labels.success, "consistent label change rejected");
    for subject in ["policy runner supported", "inventory runner supported"] {
        let row = labels
            .row("pin-diff", "info", subject)
            .ok_or_else(|| io::Error::other(format!("{subject} unreported: {:?}", labels.rows)))?;
        assert_eq!(
            row["detail"],
            "base=ubuntu-26.04 head=ubuntu-26.04,ubuntu-28.04"
        );
    }
    Ok(())
}

#[test]
fn pin_diff_fails_closed_when_a_base_parent_directory_is_a_symlink() -> TestResult {
    let fixture = Fixture::new()?;
    fs::rename(fixture.path(".velnor"), fixture.path("held-velnor"))?;
    std::os::unix::fs::symlink("held-velnor", fixture.path(".velnor"))?;
    fixture.commit_all()?;
    fs::remove_file(fixture.path(".velnor"))?;
    fs::rename(fixture.path("held-velnor"), fixture.path(".velnor"))?;
    let outcome = fixture.run(Some("HEAD"))?;
    assert!(!outcome.success, "symlinked base parent accepted");
    assert!(outcome.row("pin-diff", "fail", "HEAD").is_some());
    assert_eq!(outcome.rows_of("pin-diff", "info").len(), 0);
    Ok(())
}

#[test]
fn pin_diff_reads_the_same_base_from_a_depth_one_checkout() -> TestResult {
    let source = Fixture::new()?;
    source.commit_all()?;
    fs::write(source.path("notes.txt"), "second commit\n")?;
    source.git(&["add", "-A"])?;
    source.git(&["commit", "-q", "-m", "second"])?;
    let head = source.git_stdout(&["rev-parse", "HEAD"])?;
    let base = "refs/remotes/origin/main";

    let full = source.checkout_of_head(None)?;
    let shallow = source.checkout_of_head(Some("--depth=1"))?;
    assert_eq!(
        full.git_stdout(&["rev-parse", "--is-shallow-repository"])?,
        "false"
    );
    assert_eq!(
        shallow.git_stdout(&["rev-parse", "--is-shallow-repository"])?,
        "true"
    );
    assert!(
        full.git_stdout(&["rev-parse", "--verify", "--quiet", "HEAD^"])
            .is_ok()
    );
    assert!(
        shallow
            .git_stdout(&["rev-parse", "--verify", "--quiet", "HEAD^"])
            .is_err()
    );

    let expected = format!("unchanged vs {base} ({head})");
    for (label, checkout) in [("depth-1", &shallow), ("full", &full)] {
        let outcome = checkout.run(Some(base))?;
        assert!(
            outcome.success,
            "{label} checkout rejected: {:?}",
            outcome.rows
        );
        let rows = outcome.rows_of("pin-diff", "info");
        assert_eq!(rows.len(), 1, "{label}: unexpected rows: {rows:?}");
        assert_eq!(rows[0]["detail"], expected.as_str(), "{label}");
    }
    Ok(())
}
