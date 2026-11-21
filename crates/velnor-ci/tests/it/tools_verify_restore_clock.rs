// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;

use velnor_ci::io::cache_delete_entry::resolve_entry;
use velnor_ci::restore_clock::{SAFETY_MARGIN_SECONDS, cutoff, record};
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

fn plant(root: &std::path::Path, text: &str) {
    fs::write(root.join("velnor-tools-restore-began"), text)
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn the_recorded_start_is_used_when_it_is_earlier_than_now() {
    let job = job("restore-clock-earlier");
    record(&job.root, BEGAN).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(cutoff(&job.root, NOW), Some(margined(BEGAN)));
}

#[test]
fn a_missing_record_falls_back_to_the_time_of_the_check() {
    let job = job("restore-clock-missing");
    assert_eq!(cutoff(&job.root, NOW), Some(margined(NOW)));
}

#[test]
fn a_present_record_that_cannot_be_trusted_gives_no_cutoff_and_never_panics() {
    let job = job("restore-clock-untrusted");
    let beyond = (NOW + 1).to_string();
    let padded = format!("{BEGAN:0>4096}");
    for text in [
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
        "1790000000\n\n",
        "",
        "yesterday",
        "12.5",
        "2026-11-18T00:00:00Z",
        "99999999999999999999",
        &padded,
    ] {
        plant(&job.root, text);
        assert_eq!(cutoff(&job.root, NOW), None, "{text}");
    }
}

#[test]
fn the_oldest_and_the_newest_plausible_records_and_a_trailing_newline_are_used() {
    let job = job("restore-clock-bounds");
    for (text, began) in [
        ("1577836800", 1_577_836_800),
        (&NOW.to_string(), NOW),
        (&format!("{BEGAN}\n"), BEGAN),
    ] {
        plant(&job.root, text);
        assert_eq!(cutoff(&job.root, NOW), Some(margined(began)), "{text}");
    }
}

#[test]
fn a_record_of_exactly_64_bytes_is_used_and_one_of_65_bytes_is_refused() {
    let job = job("restore-clock-length");
    let sixty_four_zeros = format!("{BEGAN:0>64}");
    assert_eq!(sixty_four_zeros.len(), 64);
    plant(&job.root, &sixty_four_zeros);
    assert_eq!(cutoff(&job.root, NOW), Some(margined(BEGAN)));
    plant(&job.root, &format!("{BEGAN:0>63}\n"));
    assert_eq!(cutoff(&job.root, NOW), Some(margined(BEGAN)));
    plant(&job.root, &format!("{BEGAN:0>64}5"));
    assert_eq!(cutoff(&job.root, NOW), None);
    plant(&job.root, &format!("{BEGAN:0>64}\n"));
    assert_eq!(cutoff(&job.root, NOW), None);
}

#[test]
fn a_valid_time_followed_by_a_newline_and_garbage_is_refused_for_length() {
    let job = job("restore-clock-trailing-garbage");
    plant(&job.root, &format!("{BEGAN:0>63}\nGARBAGE"));
    assert_eq!(cutoff(&job.root, NOW), None);
}

#[test]
fn a_fifo_at_the_record_path_is_refused_without_blocking() {
    let job = job("restore-clock-fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(job.root.join("velnor-tools-restore-began"))
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.success());
    let root = job.root.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(cutoff(&root, NOW));
    });
    assert_eq!(
        receiver.recv_timeout(std::time::Duration::from_secs(10)),
        Ok(None)
    );
}

#[test]
fn a_record_that_is_not_a_regular_file_gives_no_cutoff() {
    let job = job("restore-clock-directory");
    fs::create_dir(job.root.join("velnor-tools-restore-began"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(cutoff(&job.root, NOW), None);
}

#[test]
fn a_record_that_is_a_symlink_gives_no_cutoff() {
    let job = job("restore-clock-symlink");
    let target = job.root.join("elsewhere");
    fs::write(&target, BEGAN.to_string()).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&target, job.root.join("velnor-tools-restore-began"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(cutoff(&job.root, NOW), None);
}

#[test]
fn an_entry_saved_inside_the_margin_before_the_start_is_refused_and_one_outside_it_resolves() {
    let job = job("restore-clock-margin");
    record(&job.root, BEGAN).unwrap_or_else(|e| unreachable!("{e}"));
    let stamp = cutoff(&job.root, NOW).unwrap_or_else(|| unreachable!("a recorded start"));
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
fn a_clock_so_early_that_the_margin_cannot_be_subtracted_gives_no_cutoff() {
    let job = job("restore-clock-early-clock");
    assert_eq!(
        cutoff(&job.root, i64::MIN + SAFETY_MARGIN_SECONDS - 1),
        None
    );
    assert_eq!(
        cutoff(&job.root, i64::MIN + SAFETY_MARGIN_SECONDS),
        Some(utc_text(i64::MIN))
    );
}

#[test]
fn an_untrusted_record_makes_the_check_print_no_command_and_ask_for_an_explicit_cutoff() {
    for (name, text) in [
        ("restore-clock-notice-min", "-9223372036854775808"),
        ("restore-clock-notice-max", "9223372036854775807"),
        ("restore-clock-notice-negative", "-5"),
        ("restore-clock-notice-garbage", "yesterday"),
        ("restore-clock-notice-future", "4000000000000"),
    ] {
        let job = job(name);
        let anchor = recorded(&job);
        fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
        plant(&job.root, text);
        let stdout = check_stdout(
            &job,
            &anchor,
            &[("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")],
        );
        assert!(
            stdout.contains(
                "the restore-time record is unreadable; rerun step 1 with an explicit --restored-before"
            ),
            "{text}: {stdout}"
        );
        assert!(!stdout.contains("cache delete-entry"), "{text}: {stdout}");
        assert!(!stdout.contains("restored-before '"), "{text}: {stdout}");
        assert!(discarded(&job), "{text}");
    }
}

#[test]
fn the_clear_step_records_the_clock_it_is_given_exactly() {
    let job = job("restore-clock-clear-exact");
    note_clear(ToolsVerb::Clear, Some(&job.root), BEGAN);
    assert_eq!(cutoff(&job.root, FAR_FUTURE), Some(margined(BEGAN)));
    let record = fs::read_to_string(job.root.join("velnor-tools-restore-began"));
    assert_eq!(record.ok().as_deref(), Some(BEGAN.to_string().as_str()));
}

#[test]
fn only_the_clear_step_records_a_start() {
    let job = job("restore-clock-other-verbs");
    for verb in [ToolsVerb::Restore, ToolsVerb::Save] {
        note_clear(verb, Some(&job.root), BEGAN);
    }
    assert_eq!(cutoff(&job.root, FAR_FUTURE), Some(margined(FAR_FUTURE)));
}

#[test]
fn a_record_path_that_is_a_symlink_is_replaced_and_its_target_is_never_written() {
    let job = job("restore-clock-write-symlink");
    let target = job.root.join("elsewhere");
    fs::write(&target, "untouched").unwrap_or_else(|e| unreachable!("{e}"));
    let path = job.root.join("velnor-tools-restore-began");
    symlink(&target, &path).unwrap_or_else(|e| unreachable!("{e}"));
    record(&job.root, BEGAN).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        fs::read_to_string(&target).ok().as_deref(),
        Some("untouched")
    );
    assert!(fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()));
    assert_eq!(cutoff(&job.root, NOW), Some(margined(BEGAN)));
}

#[test]
fn a_dangling_symlink_at_the_record_path_creates_nothing_at_its_target() {
    let job = job("restore-clock-write-dangling");
    let target = job.root.join("not-yet");
    symlink(&target, job.root.join("velnor-tools-restore-began"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    record(&job.root, BEGAN).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(fs::symlink_metadata(&target).is_err());
}

#[test]
fn the_clear_command_leaves_a_record_between_the_clock_before_and_after_it() {
    let job = job("restore-clock-clear");
    let before = now_unix();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_clear()
        .env("HOME", &job.home)
        .env("RUNNER_TEMP", &job.root)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .args(["tools-local", "clear"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let after = now_unix();
    assert!(output.status.success(), "{output:?}");
    let began: i64 = fs::read_to_string(job.root.join("velnor-tools-restore-began"))
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(|| unreachable!("the clear step left no record"));
    assert!(
        (before..=after).contains(&began),
        "{before} {began} {after}"
    );
}
