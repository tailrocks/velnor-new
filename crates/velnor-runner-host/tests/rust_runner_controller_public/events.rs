// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(super) struct Event {
    pub(super) seq: u64,
    pub(super) ms: u64,
    pub(super) label: String,
    pub(super) fields: BTreeMap<String, String>,
    pub(super) text: String,
}

impl Event {
    pub(super) fn field(&self, name: &str) -> &str {
        self.fields.get(name).map_or("", String::as_str)
    }

    pub(super) fn is_runner_start(&self) -> bool {
        self.label == "orb" && self.text.contains("/var/lib/velnor/bin/velnor-runner run")
    }

    pub(super) fn is_orb(&self, needle: &str) -> bool {
        self.label == "orb" && self.text.contains(needle)
    }
}

pub(super) fn wait_for(what: &str, seconds: u64, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn wait_unless(
    what: &str,
    seconds: u64,
    mut check: impl FnMut() -> bool,
    mut violation: impl FnMut() -> Option<String>,
) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        assert_eq!(
            violation(),
            None,
            "behaviour broken while waiting for {what}"
        );
        if check() {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn parse_events(path: &Path) -> Vec<Event> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines().filter_map(parse_event).collect()
}

fn parse_event(line: &str) -> Option<Event> {
    let mut parts = line.splitn(4, ' ');
    let seq = parts.next()?.parse().ok()?;
    let ms = parts.next()?.parse().unwrap_or(0);
    let kind = parts.next()?;
    let rest = parts.next().unwrap_or("");
    if kind == "ORB" {
        return Some(Event {
            seq,
            ms,
            label: "orb".to_owned(),
            fields: BTreeMap::new(),
            text: rest.to_owned(),
        });
    }
    let (label, detail) = rest.split_once(' ').unwrap_or((rest, ""));
    let fields = detail
        .split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    Some(Event {
        seq,
        ms,
        label: label.to_owned(),
        fields,
        text: detail.to_owned(),
    })
}

pub(super) fn files_containing(root: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                pending.extend(entries.filter_map(Result::ok).map(|entry| entry.path()));
            }
        } else if metadata.is_file()
            && std::fs::read(&path).is_ok_and(|bytes| {
                bytes
                    .windows(needle.len())
                    .any(|window| window == needle.as_bytes())
            })
        {
            found.push(path);
        }
    }
    found
}

pub(super) fn process_exists(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
