// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use velnor_ci::io::cache_delete_entry::resolve_entry;
use velnor_ci::restore_clock::{ENV_NAME, SAFETY_MARGIN_SECONDS, cutoff, record};
use velnor_ci::time::{now_unix, parse_utc, utc_text};
use velnor_ci::tools_local::{Verb as ToolsVerb, note_clear};

use crate::cache_delete_entry::{KEY, MAIN, VERSION_A, api, entry_saved, refs, repo};
use crate::tools_verify::{discarded, job, recorded, rustc};
use crate::tools_verify_notice::check_stdout;

const BEGAN: i64 = 1_790_000_000;
const NOW: i64 = BEGAN + 90;
const FAR_FUTURE: i64 = 4_000_000_000_000;

fn margined(seconds: i64) -> String {
    utc_text(seconds - SAFETY_MARGIN_SECONDS)
}

fn text(seconds: i64) -> String {
    seconds.to_string()
}

#[test]
fn a_missing_value_falls_back_to_the_time_of_the_check() {
    assert_eq!(cutoff(None, NOW), Some(margined(NOW)));
}

#[test]
fn a_present_value_that_cannot_be_trusted_gives_no_cutoff_and_never_panics() {
    let beyond = text(NOW + 1);
    let padded = format!("{BEGAN:0>4096}");
    for value in [
        "-9223372036854775808",
        "9223372036854775807",
        "9223372036854775808",
        "-5",
        "-1790000000",
        "0",
        "1000",
        "1577836799",
        "4000000000000",
        &beyond,
        "+1790000000",
        " 1790000000",
        "1790000000 ",
        "1790000000\n",
        "",
        "yesterday",
        "12.5",
        "2026-11-18T00:00:00Z",
        "99999999999999999999",
        &padded,
    ] {
        assert_eq!(cutoff(Some(value), NOW), None, "{value}");
    }
}

#[test]
fn the_oldest_and_the_newest_plausible_values_are_used() {
    for began in [1_577_836_800, NOW] {
        assert_eq!(
            cutoff(Some(&text(began)), NOW),
            Some(margined(began)),
            "{began}"
        );
    }
}

#[test]
fn an_entry_saved_inside_the_margin_before_the_start_is_refused_and_one_outside_it_resolves() {
    let stamp = cutoff(Some(&text(BEGAN)), NOW).unwrap_or_else(|| unreachable!("a recorded start"));
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
    let stdout = check_stdout(
        &job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
            (ENV_NAME, &text(began)),
        ],
    );
    assert!(
        stdout.contains("--restored-before '2025-12-31T23:55:00Z'"),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "the time this job's restore began, taken from the value that `tools-local clear` exports to the later steps before the restores"
        ) && stdout.contains("minus a safety margin of 5 minutes"),
        "{stdout}"
    );
}

#[test]
fn a_clock_so_early_that_the_margin_cannot_be_subtracted_gives_no_cutoff() {
    assert_eq!(cutoff(None, i64::MIN + SAFETY_MARGIN_SECONDS - 1), None);
    assert_eq!(
        cutoff(None, i64::MIN + SAFETY_MARGIN_SECONDS),
        Some(utc_text(i64::MIN))
    );
}

#[test]
fn an_untrusted_value_makes_the_check_print_no_command_and_ask_for_an_explicit_cutoff() {
    for (name, value) in [
        ("restore-clock-notice-min", "-9223372036854775808"),
        ("restore-clock-notice-max", "9223372036854775807"),
        ("restore-clock-notice-negative", "-5"),
        ("restore-clock-notice-garbage", "yesterday"),
        ("restore-clock-notice-future", "4000000000000"),
        ("restore-clock-notice-empty", ""),
    ] {
        let job = job(name);
        let anchor = recorded(&job);
        fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
        let stdout = check_stdout(
            &job,
            &anchor,
            &[
                ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
                (ENV_NAME, value),
            ],
        );
        assert!(
            stdout.contains(
                "the restore-time value is unreadable; rerun step 1 with an explicit --restored-before"
            ),
            "{value}: {stdout}"
        );
        assert!(!stdout.contains("cache delete-entry"), "{value}: {stdout}");
        assert!(!stdout.contains("restored-before '"), "{value}: {stdout}");
        assert!(discarded(&job), "{value}");
    }
}

#[test]
fn the_clear_step_exports_the_clock_it_is_given_exactly() {
    let job = job("restore-clock-clear-exact");
    let file = job.root.join("github-env");
    fs::write(&file, "EARLIER=1\n").unwrap_or_else(|e| unreachable!("{e}"));
    note_clear(ToolsVerb::Clear, Some(&file), BEGAN);
    assert_eq!(
        fs::read_to_string(&file).ok().as_deref(),
        Some(format!("EARLIER=1\n{ENV_NAME}={BEGAN}\n").as_str())
    );
}

#[test]
fn only_the_clear_step_exports_a_start() {
    let job = job("restore-clock-other-verbs");
    let file = job.root.join("github-env");
    for verb in [ToolsVerb::Restore, ToolsVerb::Save] {
        note_clear(verb, Some(&file), BEGAN);
    }
    assert!(!file.exists());
    assert_eq!(cutoff(None, FAR_FUTURE), Some(margined(FAR_FUTURE)));
}

#[test]
fn a_record_that_cannot_be_written_is_reported_and_the_clear_step_goes_on() {
    let job = job("restore-clock-unwritable");
    let blocked = job.root.join("no-such-dir").join("github-env");
    assert!(record(&blocked, BEGAN).is_err());
    note_clear(ToolsVerb::Clear, Some(&blocked), BEGAN);
}

#[test]
fn the_clear_command_exports_a_start_between_the_clock_before_and_after_it() {
    let job = job("restore-clock-clear");
    let file = job.root.join("github-env");
    let before = now_unix();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_clear()
        .env("HOME", &job.home)
        .env("RUNNER_TEMP", &job.root)
        .env("GITHUB_ENV", &file)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .args(["tools-local", "clear"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let after = now_unix();
    assert!(output.status.success(), "{output:?}");
    let began: i64 = fs::read_to_string(&file)
        .ok()
        .and_then(|text| {
            text.strip_prefix(&format!("{ENV_NAME}="))
                .and_then(|value| value.strip_suffix('\n'))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| unreachable!("the clear step exported no start"));
    assert!(
        (before..=after).contains(&began),
        "{before} {began} {after}"
    );
}

#[test]
fn a_file_an_archive_member_plants_in_runner_temp_cannot_move_the_cutoff() {
    let job = job("restore-clock-planted");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let began = parse_utc("2026-01-01T00:00:00Z").unwrap_or_else(|| unreachable!("fixed time"));
    let forged = parse_utc("2026-02-01T00:00:00Z").unwrap_or_else(|| unreachable!("fixed time"));
    fs::write(
        job.root.join("velnor-tools-restore-began"),
        forged.to_string(),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
            (ENV_NAME, &text(began)),
        ],
    );
    assert!(
        stdout.contains("--restored-before '2025-12-31T23:55:00Z'"),
        "{stdout}"
    );
    assert!(!stdout.contains("2026-01-31"), "{stdout}");
}
