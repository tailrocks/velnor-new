// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::harness::{Event, World, wait_for};

pub(super) struct Snapshot {
    pub(super) jit: Vec<String>,
    pub(super) work: Vec<String>,
    pub(super) runners: Vec<String>,
    pub(super) ps: String,
}

pub(super) fn assert_clean(world: &World, capacity: &str) {
    assert!(
        !world.seen.join("unexpected").exists() && !world.seen.join("fake-failure").exists(),
        "the fake GitHub saw a request outside its table or failed"
    );
    for poll in world.gh("poll") {
        assert_eq!(poll.field("capacity"), capacity, "poll {}", poll.seq);
    }
    assert!(
        !world.state.join("launch.db").exists(),
        "launch.db was created"
    );
}

pub(super) fn reservation(event: &Event) -> u64 {
    event
        .field("name")
        .rsplit('-')
        .next()
        .and_then(|number| number.parse().ok())
        .unwrap_or_default()
}

pub(super) fn jit_name(reservation: u64) -> String {
    format!("{reservation}.jit")
}

pub(super) fn runner_name(reservation: u64) -> String {
    format!("velnor-mac-{reservation}")
}

pub(super) fn snapshot(world: &World, acquire: &Event) -> Snapshot {
    let text = std::fs::read_to_string(world.seen.join(format!("snapshots/{}.txt", acquire.seq)))
        .unwrap_or_default();
    let mut lines: BTreeMap<&str, &str> = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(": ") {
            lines.insert(key, value);
        }
    }
    let list = |key: &str| -> Vec<String> {
        lines.get(key).map_or_else(Vec::new, |value| {
            value
                .split(',')
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect()
        })
    };
    Snapshot {
        jit: list("jit"),
        work: list("work"),
        runners: list("runners"),
        ps: text
            .split_once("ps:\n")
            .map_or(String::new(), |(_, ps)| ps.to_owned()),
    }
}

pub(super) fn acquire_ids(world: &World) -> Vec<String> {
    world
        .gh("acquire")
        .iter()
        .map(|event| event.field("ids").to_owned())
        .collect()
}

pub(super) fn deleted_messages(world: &World) -> Vec<String> {
    world
        .gh("message.delete")
        .iter()
        .map(|event| event.field("id").to_owned())
        .collect()
}

pub(super) fn heartbeat_advances(world: &World, count: usize) {
    let mut last = world.heartbeat_ms().unwrap_or_default();
    let mut seen = 0;
    wait_for("heartbeat advances", 30, || {
        let now = world.heartbeat_ms().unwrap_or_default();
        if now > last {
            seen += 1;
            last = now;
        }
        seen >= count
    });
}

pub(super) fn orb_calls_after(world: &World, seq: u64, needle: &str) -> Vec<Event> {
    world
        .events()
        .into_iter()
        .filter(|event| event.seq > seq && event.is_orb(needle))
        .collect()
}

pub(super) fn call_number(event: &Event) -> String {
    event
        .text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_owned()
}
