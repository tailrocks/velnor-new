// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;

use velnor_ci::io::cache_delete_entry::resolve_entry;
use velnor_ci::restore_clock::{SAFETY_MARGIN_SECONDS, cutoff, record};
use velnor_ci::time::{parse_utc, utc_text};

use crate::cache_delete_entry::{KEY, MAIN, VERSION_A, api, entry_saved, refs, repo};
use crate::tools_verify::{job, recorded, rustc};
use crate::tools_verify_notice::check_stdout;

const BEGAN: i64 = 1_790_000_000;
const NOW: i64 = BEGAN + 90;
const FAR_FUTURE: i64 = 4_000_000_000_000;

fn margined(seconds: i64) -> String {
    utc_text(seconds - SAFETY_MARGIN_SECONDS)
}

#[test]
fn the_recorded_start_is_used_when_it_is_earlier_than_now() {
    let job = job("restore-clock-earlier");
    record(&job.root, BEGAN).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(cutoff(&job.root, NOW), margined(BEGAN));
}

#[test]
fn a_recorded_start_in_the_future_is_ignored() {
    let job = job("restore-clock-future");
    record(&job.root, NOW + 3_600).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(cutoff(&job.root, NOW), margined(NOW));
}

#[test]
fn a_missing_or_unparsable_record_falls_back_to_now() {
    let job = job("restore-clock-unparsable");
    assert_eq!(cutoff(&job.root, NOW), margined(NOW));
    for text in [
        "",
        "yesterday",
        "12.5",
        "2026-11-18T00:00:00Z",
        "99999999999999999999",
    ] {
        fs::write(job.root.join("velnor-tools-restore-began"), text)
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(cutoff(&job.root, NOW), margined(NOW), "{text}");
    }
}

#[test]
fn a_record_that_is_a_symlink_is_not_followed() {
    let job = job("restore-clock-symlink");
    let target = job.root.join("elsewhere");
    fs::write(&target, BEGAN.to_string()).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&target, job.root.join("velnor-tools-restore-began"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(cutoff(&job.root, NOW), margined(NOW));
}

#[test]
fn an_entry_saved_inside_the_margin_before_the_start_is_refused_and_one_outside_it_resolves() {
    let job = job("restore-clock-margin");
    record(&job.root, BEGAN).unwrap_or_else(|e| unreachable!("{e}"));
    let stamp = cutoff(&job.root, NOW);
    for (id, saved, resolves) in [
        (21, BEGAN - 1, false),
        (22, BEGAN - SAFETY_MARGIN_SECONDS, false),
        (23, BEGAN - SAFETY_MARGIN_SECONDS - 1, true),
    ] {
        let api = api(vec![entry_saved(
            id,
            KEY,
            MAIN,
            VERSION_A,
            &utc_text(saved),
        )]);
        let found = resolve_entry(&api, &repo(), KEY, &refs(&[MAIN]), &stamp);
        assert_eq!(found.is_ok(), resolves, "saved {saved}: {found:?}");
    }
}

#[test]
fn the_check_prints_the_recorded_start_minus_the_margin() {
    let job = job("restore-clock-notice");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let began = parse_utc("2026-01-01T00:00:00Z").unwrap_or_else(|| unreachable!("fixed time"));
    record(&job.root, began).unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")],
    );
    assert!(
        stdout.contains("--restored-before '2025-12-31T23:55:00Z'"),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "the time this job's restore began, taken from the record that `tools-local clear` leaves before the restores"
        ) && stdout.contains("minus a safety margin of 5 minutes"),
        "{stdout}"
    );
}

#[test]
fn the_clear_step_records_its_start_for_the_check() {
    let job = job("restore-clock-clear");
    assert_eq!(cutoff(&job.root, FAR_FUTURE), margined(FAR_FUTURE));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_clear()
        .env("HOME", &job.home)
        .env("RUNNER_TEMP", &job.root)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .args(["tools-local", "clear"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    assert_ne!(cutoff(&job.root, FAR_FUTURE), margined(FAR_FUTURE));
}
