// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_delete_entry_cli::{
    FAIL, KEY, MAIN, MERGE, RESTORED_BEFORE, VERSION_A, VERSION_B, delete_entry, deleted, entry,
    serve, step_one,
};

#[test]
fn the_binary_resolves_the_first_ref_when_a_later_ref_holds_the_same_version() {
    let (api, served) = serve(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_A),
    ]);
    let output = step_one(&api, &["--key", KEY, "--ref", MERGE, "--ref", MAIN]);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("--id 12"));
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn the_binary_refuses_a_key_that_only_a_later_ref_holds_and_prints_no_id_command() {
    let (api, served) = serve(vec![entry(11, KEY, MAIN, VERSION_A)]);
    let output = step_one(&api, &["--key", KEY, "--ref", MERGE, "--ref", MAIN]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(
        stderr.contains(&format!("is no longer on {MERGE}"))
            && stderr.contains(&format!("re-run with `--ref {MAIN}` first")),
        "{stderr}"
    );
    assert!(
        !stdout.contains("--id") && !stderr.contains("--id"),
        "{stdout}{stderr}"
    );
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn the_binary_respects_the_order_of_the_refs() {
    let (api, served) = serve(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_A),
    ]);
    let output = step_one(&api, &["--key", KEY, "--ref", MAIN, "--ref", MERGE]);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("--id 11"));
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn the_binary_refuses_when_the_listing_of_the_first_ref_fails() {
    let (api, served) = serve(vec![entry(11, KEY, MAIN, VERSION_A)]);
    let output = step_one(&api, &["--key", KEY, "--ref", FAIL, "--ref", MAIN]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("502"), "{stderr}");
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn the_binary_refuses_a_version_split_across_refs_and_names_both_entries() {
    let (api, served) = serve(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_B),
    ]);
    let output = step_one(&api, &["--key", KEY, "--ref", MERGE, "--ref", MAIN]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!("ID 12 version {VERSION_B}"))
            && stderr.contains(&format!("ID 11 version {VERSION_A}")),
        "{stderr}"
    );
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn step_one_without_restored_before_is_refused_by_the_parser() {
    let (api, served) = serve(vec![entry(12, KEY, MERGE, VERSION_B)]);
    let output = delete_entry(&api, &["--key", KEY, "--ref", MERGE]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("the following required arguments were not provided")
            && stderr.contains("--restored-before <RESTORED_BEFORE>"),
        "{stderr}"
    );
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn an_id_together_with_restored_before_is_refused_by_the_parser() {
    let (api, served) = serve(vec![entry(12, KEY, MERGE, VERSION_B)]);
    let output = delete_entry(
        &api,
        &[
            "--key",
            KEY,
            "--id",
            "12",
            "--restored-before",
            RESTORED_BEFORE,
        ],
    );
    assert!(!output.status.success());
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn the_binary_refuses_an_entry_saved_after_the_stamp_and_prints_no_id_command() {
    let (api, served) = serve(vec![entry(13, KEY, MERGE, VERSION_B)]);
    let output = delete_entry(
        &api,
        &[
            "--key",
            KEY,
            "--ref",
            MERGE,
            "--restored-before",
            "2026-11-17T00:00:00Z",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(
        stderr.contains("saved within 5 minutes before this run's restore or after it")
            && stderr.contains("--restored-before '2026-11-17T00:00:01Z'"),
        "{stderr}"
    );
    assert!(!stdout.contains("--id"), "{stdout}");
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn the_printed_id_command_repeats_a_non_default_api_url() {
    let (api, _) = serve(vec![entry(12, KEY, MERGE, VERSION_B)]);
    let output = step_one(&api, &["--key", KEY, "--ref", MERGE]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "GITHUB_API_URL='{api}' velnor-ci cache delete-entry"
        )),
        "{stdout}"
    );
}
