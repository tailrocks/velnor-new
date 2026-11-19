// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `velnor-ci resolve-qualification` against what the legacy `resolve-qualification-v1` op does
//! in the same recorded world: the same API requests in the same order, the same admission file
//! byte for byte, the same refusal. `data/resolve_qualification/expected/<case>/` holds the
//! recorded legacy outcome; `record_the_legacy_op` re-records it from the engine.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::resolve_cases::cases;
use crate::resolve_run::{Outcome, last_micros, run_helper, run_legacy};
use crate::resolve_world::{Quirk, World, data_dir};

const BIN: &str = env!("CARGO_BIN_EXE_velnor-ci");

fn expected_dir(name: &str) -> PathBuf {
    data_dir().join("expected").join(name)
}

fn read_expected(name: &str) -> Outcome {
    let dir = expected_dir(name);
    let text = |file: &str| fs::read_to_string(dir.join(file)).unwrap_or_default();
    Outcome {
        error: text("error"),
        requests: text("requests").lines().map(str::to_owned).collect(),
        admission: fs::read(dir.join("admission")).ok(),
    }
}

fn write_expected(name: &str, outcome: &Outcome) {
    let dir = expected_dir(name);
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    for file in ["error", "requests", "admission"] {
        fs::remove_file(dir.join(file)).unwrap_or_default();
    }
    if !outcome.error.is_empty() {
        fs::write(dir.join("error"), &outcome.error).unwrap_or_else(|e| unreachable!("{e}"));
    }
    if !outcome.requests.is_empty() {
        fs::write(dir.join("requests"), outcome.requests.join("\n") + "\n")
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    if let Some(bytes) = &outcome.admission {
        fs::write(dir.join("admission"), bytes).unwrap_or_else(|e| unreachable!("{e}"));
    }
}

#[test]
fn every_recorded_case_matches_the_legacy_op() {
    let all = cases();
    assert!(all.len() >= 110, "{} cases", all.len());
    let mut wrong = Vec::new();
    for (name, mut world) in all {
        let got = run_helper(&mut world, BIN);
        let want = read_expected(name);
        if got != want {
            wrong.push(format!("{name}: want {want:?}, got {got:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_recorded_outcomes_cover_every_phase_and_the_refusals() {
    let outcomes: Vec<(&str, Outcome)> = cases()
        .into_iter()
        .map(|(name, _)| (name, read_expected(name)))
        .collect();
    let staged = outcomes
        .iter()
        .filter(|(_, o)| o.admission.is_some())
        .count();
    assert!(staged >= 5, "{staged} cases stage an admission file");
    let errors: BTreeSet<&str> = outcomes
        .iter()
        .map(|(_, o)| o.error.as_str())
        .filter(|error| !error.is_empty())
        .collect();
    assert!(errors.len() >= 60, "{} distinct refusals", errors.len());
    for code in [
        "qualification_run_authority_mismatch",
        "qualification_artifact_integrity",
        "qualification_source_not_ancestor",
        "qualification_api_unavailable",
        "qualification_default_branch_unprotected",
        "predecessor_receipt_digest_mismatch",
        "qualification_artifact_link_mismatch",
        "qualification_request_path_mismatch",
    ] {
        assert!(
            errors.iter().any(|error| error.contains(code)),
            "no recorded refusal {code}"
        );
    }
}

#[test]
fn a_phase_without_a_predecessor_asks_only_for_the_repository_and_its_branch() {
    for name in ["cold-has-no-predecessor", "control-has-no-predecessor"] {
        let want = read_expected(name);
        assert_eq!(want.requests.len(), 2, "{name}");
        assert!(want.admission.is_none(), "{name}");
    }
}

fn helper_error(world: &mut World) -> String {
    run_helper(world, BIN).error
}

#[test]
fn the_helper_reads_the_file_write_request_writes_where_the_legacy_op_refuses_it() {
    let legacy = fs::read_to_string(data_dir().join("legacy_refuses_write_request_name"))
        .unwrap_or_default();
    assert_eq!(legacy, "qualification_request_path_mismatch");
    assert_eq!(helper_error(&mut World::new("warm")), "");
}

#[test]
fn a_run_without_a_token_an_event_or_a_checkout_is_refused() {
    let mut world = World::new("warm");
    world.unset.push("GH_TOKEN");
    assert_eq!(helper_error(&mut world), "missing_github_token");
    let mut world = World::new("warm");
    world
        .env
        .insert("GITHUB_EVENT_NAME".to_owned(), "push".to_owned());
    assert_eq!(helper_error(&mut world), "qualification_event_mismatch");
    let mut world = World::new("warm");
    world.quirk = Some(Quirk::NotARepository);
    assert!(helper_error(&mut world).starts_with("not_work_tree: not_inside_work_tree:"));
}

#[test]
fn an_event_payload_that_cannot_be_read_is_refused_before_anything_is_asked() {
    let mut world = World::new("warm");
    world
        .env
        .insert("GITHUB_EVENT_PATH".to_owned(), "/nonexistent".to_owned());
    let got = run_helper(&mut world, BIN);
    assert_eq!(got.error, "unreadable_event_payload");
    assert_eq!(got.requests, Vec::<String>::new());
    let mut world = World::new("warm");
    world.payload_bytes = Some(b"{".to_vec());
    assert_eq!(helper_error(&mut world), "malformed_event_payload");
}

/// Record the legacy engine's outcome for every case:
/// `VELNOR_LEGACY_ENGINE=<velnor-actions built from this repository> cargo test --test it record_the_legacy_op -- --ignored`
/// (`VELNOR_RECORD_ONLY=<part of a case name>` records only the cases whose name contains it)
#[test]
#[ignore = "needs the legacy engine in VELNOR_LEGACY_ENGINE"]
fn record_the_legacy_op() {
    let engine = std::env::var("VELNOR_LEGACY_ENGINE")
        .unwrap_or_else(|_| unreachable!("set VELNOR_LEGACY_ENGINE"));
    let only = std::env::var("VELNOR_RECORD_ONLY").unwrap_or_default();
    for (name, mut world) in cases() {
        if name.contains(&only) {
            write_expected(name, &run_legacy(&mut world, &engine));
        }
    }
    let mut world = World::new("warm");
    world.request_name = Some("plan-v1-request.json".to_owned());
    fs::write(
        data_dir().join("legacy_refuses_write_request_name"),
        run_legacy(&mut world, &engine).error,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
}

/// `VELNOR_LEGACY_ENGINE=<engine> cargo test --test it time_the_legacy_op_and_the_helper -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, needs the legacy engine in VELNOR_LEGACY_ENGINE"]
fn time_the_legacy_op_and_the_helper() {
    let engine = std::env::var("VELNOR_LEGACY_ENGINE")
        .unwrap_or_else(|_| unreachable!("set VELNOR_LEGACY_ENGINE"));
    for phase in ["warm", "useful_delta"] {
        let (mut legacy, mut helper) = (Vec::new(), Vec::new());
        for _ in 0..5 {
            run_legacy(&mut World::new(phase), &engine);
            legacy.push(last_micros());
            run_helper(&mut World::new(phase), BIN);
            helper.push(last_micros());
        }
        legacy.sort_unstable();
        helper.sort_unstable();
        println!(
            "{phase}: legacy {legacy:?} us, helper {helper:?} us (wall of the process, median {} ms vs {} ms)",
            legacy[2] / 1000,
            helper[2] / 1000
        );
    }
}
