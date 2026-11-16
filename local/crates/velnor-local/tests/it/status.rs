// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::pairs::decisions;
use velnor_local::status::{Snapshot, pair_states, render};

const PLAN: &str = r#"{"sides":[{"side":"trusted","network":"n","pairs":2},{"side":"untrusted","network":"n-fork","pairs":1}]}"#;
const ADMITTED: &str = r#"{"pair":"trusted-1","side":"trusted","allow":true,"reason":null,"ledger":"1-push","run_id":4,"job":"ci"}"#;
const REFUSED: &str = r#"{"pair":"trusted-2","side":"trusted","allow":false,"reason":"not_dispatched","ledger":null,"run_id":5,"job":"lint"}"#;

fn states(log: &str, lines: &[&str]) -> Vec<String> {
    let found = decisions(&lines.join("\n")).unwrap_or_else(|e| unreachable!("{e}"));
    pair_states(PLAN, log, &found).unwrap_or_else(|e| unreachable!("{e}"))
}

const LISTENING: &str =
    "pair trusted-1 listening\npair trusted-2 listening\npair untrusted-1 listening\n";

#[test]
fn a_pair_that_took_a_job_is_busy_with_its_run_and_job_and_the_others_are_idle() {
    assert_eq!(
        states(LISTENING, &[ADMITTED]),
        [
            "trusted-1 busy (run 4, job ci)",
            "trusted-2 idle",
            "untrusted-1 idle"
        ]
    );
}

#[test]
fn a_pair_that_was_removed_after_its_job_is_idle_again() {
    let log = format!("{LISTENING}pair trusted-1 removed\n");
    assert_eq!(
        states(&log, &[ADMITTED]),
        ["trusted-1 idle", "trusted-2 idle", "untrusted-1 idle"]
    );
    let replaced = format!("{log}pair trusted-1 listening\n");
    assert_eq!(states(&replaced, &[ADMITTED])[0], "trusted-1 idle");
}

#[test]
fn a_refused_job_leaves_no_busy_pair() {
    assert_eq!(states(LISTENING, &[REFUSED])[1], "trusted-2 idle");
}

fn snapshot(pairs: Vec<String>, refusal: Option<&str>) -> Snapshot {
    Snapshot {
        name: "velnor-local-2c41a9f0".to_owned(),
        up_seconds: 14 * 60 + 5,
        mode: "pair",
        authority: "127.0.0.1:53114".to_owned(),
        ledger: 6,
        pairs,
        refusal: refusal.map(str::to_owned),
    }
}

#[test]
fn the_status_shows_mode_authority_ledger_pairs_and_the_last_refusal() {
    let text = render(&snapshot(
        states(LISTENING, &[ADMITTED, REFUSED]),
        Some("run 5 job lint: not_dispatched"),
    ));
    assert_eq!(
        text,
        "velnor-local-2c41a9f0: up 14 min, runner mode pair, authority 127.0.0.1:53114, ledger 6 events\n  pairs: trusted-1 busy (run 4, job ci)  trusted-2 idle  untrusted-1 idle\n  last refusal: run 5 job lint: not_dispatched\n"
    );
}

#[test]
fn plain_mode_has_no_pairs_and_no_refusal_reads_none() {
    let mut plain = snapshot(Vec::new(), None);
    plain.mode = "plain";
    plain.ledger = 1;
    plain.up_seconds = 42;
    assert_eq!(
        render(&plain),
        "velnor-local-2c41a9f0: up 42 s, runner mode plain, authority 127.0.0.1:53114, ledger 1 event\n  pairs: none (plain containers)\n  last refusal: none\n"
    );
}

#[test]
fn a_plan_that_is_not_json_is_an_error() {
    assert!(pair_states("nope", "", &[]).is_err());
}

fn state(name: &str, pid: &str) -> std::path::PathBuf {
    let dir = crate::support::temp_dir(name);
    let write = |path: &str, text: &str| {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap_or(&dir))
            .unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(file, text).unwrap_or_else(|e| unreachable!("{e}"));
    };
    write("authority/pid", pid);
    write("authority/address", "127.0.0.1:53114\n");
    write("authority/ledger.jsonl", "{}\n{}\n{}\n");
    write("runner/plan.json", PLAN);
    write("runner/velnor-host.log", LISTENING);
    write(
        "runner/decisions.jsonl",
        &format!("{ADMITTED}\n{REFUSED}\n"),
    );
    dir
}

fn command(dir: &std::path::Path) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_velnor-local"))
        .args(["status", "--dir"])
        .arg(dir)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn the_command_reads_the_state_directory_of_a_running_environment() {
    let dir = state("status-up", &std::process::id().to_string());
    let output = command(&dir);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("runner mode pair, authority 127.0.0.1:53114, ledger 3 events"),
        "{text}"
    );
    assert!(
        text.contains("pairs: trusted-1 busy (run 4, job ci)  trusted-2 idle  untrusted-1 idle"),
        "{text}"
    );
    assert!(
        text.contains("last refusal: run 5 job lint: not_dispatched"),
        "{text}"
    );
}

#[test]
fn the_command_fails_and_names_the_fix_when_the_authority_is_not_running() {
    let dir = state("status-down", "2147483646");
    let output = command(&dir);
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        error.contains("is not up") && error.contains("velnor-local up --dir"),
        "{error}"
    );
}
