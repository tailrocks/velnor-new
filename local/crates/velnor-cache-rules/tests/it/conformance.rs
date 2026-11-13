// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_cache_rules::{Entry, Event, Scope, Timestamp, access_for, lookup, reserve};

fn observation(probe: &str) -> Value {
    let path = format!(
        "{}/../../conformance/{probe}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path);
    assert!(text.is_ok(), "{path}");
    let parsed = serde_json::from_str(&text.unwrap_or_default());
    assert!(parsed.is_ok(), "{path}");
    parsed.unwrap_or_default()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn entry(scope: Scope, key: &str, created: u64) -> Entry {
    Entry {
        scope,
        key: key.to_owned(),
        version: "v1".to_owned(),
        size_bytes: 1,
        created: Timestamp(created),
        last_access: Timestamp(created),
    }
}

fn push(branch: &str) -> Event {
    Event::Push {
        branch: branch.to_owned(),
    }
}

fn pull(number: u64, base: &str) -> Event {
    Event::PullRequest {
        number,
        base_branch: base.to_owned(),
    }
}

fn matched(entries: &[Entry], event: &Event, key: &str, restore_keys: &[&str]) -> String {
    let keys: Vec<String> = restore_keys.iter().map(|k| (*k).to_owned()).collect();
    lookup(entries, &access_for(event, "main"), key, &keys, "v1")
        .map_or_else(String::new, |found| found.key.clone())
}

#[test]
fn token_scopes_match_the_access_of_each_recorded_event() {
    let document = observation("token");
    let cases = [
        ("push", push("probe-<tag>-events")),
        ("pull_request", pull(7, "main")),
    ];
    for (name, event) in cases {
        let access = access_for(&event, "main");
        let expected: Vec<(u64, String)> = access
            .read
            .iter()
            .map(|scope| {
                let permission = if *scope == access.write { 3 } else { 1 };
                (permission, scope.0.replace("/7/", "/<n>/"))
            })
            .collect();
        let recorded: Vec<(u64, String)> = document["legs"][name]["cache_scopes"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        (
                            item["permission"].as_u64().unwrap_or(0),
                            text(&item["scope"]).to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(recorded, expected, "{name}");
    }
}

#[test]
fn restore_keys_pick_what_github_picked() {
    let leg = observation("cache-restore-keys")["legs"]["default-branch"].clone();
    let main = Scope::branch("main");
    let entries = [
        entry(main.clone(), "lab-probe-keys-<tag>-b", 1),
        entry(main, "lab-probe-keys-<tag>-a", 2),
    ];
    let event = push("main");
    let none = "lab-probe-keys-<tag>-none";
    let cases: [(&str, &str, Vec<&str>); 5] = [
        ("exact", "lab-probe-keys-<tag>-b", vec![]),
        ("key_prefix", "lab-probe-keys-<tag>-", vec![]),
        ("restore_key_prefix", none, vec!["lab-probe-keys-<tag>-"]),
        (
            "restore_key_order",
            none,
            vec!["lab-probe-keys-<tag>-b", "lab-probe-keys-<tag>-a"],
        ),
        ("no_match", none, vec!["lab-probe-keys-<tag>x"]),
    ];
    for (name, key, restore_keys) in cases {
        let found = matched(&entries, &event, key, &restore_keys);
        assert_eq!(found, text(&leg[name]["matched"]), "{name}");
    }
}

#[test]
fn a_pull_request_reads_default_and_base_branches_but_not_its_head_branch() {
    let leg = observation("cache-pr-reads")["legs"]["pull_request"].clone();
    let entries = [
        entry(Scope::branch("main"), "lab-probe-reads-<tag>-default", 1),
        entry(
            Scope::branch("probe-<tag>-base"),
            "lab-probe-reads-<tag>-base",
            2,
        ),
        entry(
            Scope::branch("probe-<tag>-head"),
            "lab-probe-reads-<tag>-head",
            3,
        ),
    ];
    let event = pull(5, "probe-<tag>-base");
    for (name, key) in [
        ("default_branch", "lab-probe-reads-<tag>-default"),
        ("base_branch", "lab-probe-reads-<tag>-base"),
        ("head_branch", "lab-probe-reads-<tag>-head"),
    ] {
        let found = matched(&entries, &event, key, &[]);
        assert_eq!(found, text(&leg[name]["matched"]), "{name}");
    }
}

#[test]
fn a_pull_request_cannot_read_what_another_pull_request_saved() {
    let leg = observation("cache-other-pr")["legs"]["second_pull_request"].clone();
    let entries = [entry(Scope::pull_merge(1), "lab-probe-other-<tag>-a", 1)];
    let event = pull(2, "main");
    let exact = matched(&entries, &event, "lab-probe-other-<tag>-a", &[]);
    assert_eq!(exact, text(&leg["exact_key_of_the_first"]["matched"]));
    let prefix = matched(
        &entries,
        &event,
        "lab-probe-other-<tag>-none",
        &["lab-probe-other-<tag>-"],
    );
    assert_eq!(prefix, text(&leg["restore_key_prefix"]["matched"]));
}

#[test]
fn a_second_run_of_a_pull_request_restores_what_its_first_run_saved() {
    let document = observation("cache-own-pr");
    let event = pull(3, "main");
    let access = access_for(&event, "main");
    let key = "lab-probe-own-<tag>";
    let mut entries = Vec::new();
    for name in ["run-1", "run-2"] {
        let leg = &document["legs"][name];
        let found = matched(&entries, &event, key, &[]);
        let saved = found.is_empty() && reserve(&entries, &access, key, "v1").is_ok();
        if saved {
            entries.push(entry(access.write.clone(), key, 1));
        }
        assert_eq!(found, text(&leg["restore"]["matched"]), "{name}");
        assert_eq!(Some(saved), leg["saved"].as_bool(), "{name}");
    }
}

#[test]
fn saving_an_existing_key_is_refused_and_the_step_goes_on() {
    let leg = observation("cache-save-existing")["legs"]["default-branch"].clone();
    let main = Scope::branch("main");
    let key = "lab-probe-existing-<tag>";
    let access = access_for(&push("main"), "main");
    assert!(reserve(&[], &access, key, "v1").is_ok());
    assert!(reserve(&[entry(main, key, 1)], &access, key, "v1").is_err());
    let outputs = &leg["step_output"];
    assert!(text(&outputs[0][0]).starts_with("Cache saved with key"));
    assert!(text(&outputs[1][0]).starts_with("Failed to save: Unable to reserve cache"));
    assert_eq!(text(&leg["second_save_conclusion"]), "success");
}
