// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;

use crate::inputs::{SavedInput, is_representable};

const VERSION: &str = "velnor-input-dirs-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    File,
    Directory,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry {
    pub name: OsString,
    pub kind: EntryKind,
}

pub type Listings = BTreeMap<String, Vec<Entry>>;

#[must_use]
pub fn directories(files: &[SavedInput]) -> BTreeSet<String> {
    let parents: HashSet<&str> = files
        .iter()
        .filter_map(|input| input.file.path.rsplit_once('/').map(|(parent, _)| parent))
        .collect();
    let mut directories = BTreeSet::new();
    for parent in parents {
        let mut directory = parent;
        while !directory.is_empty() && directories.insert(directory.to_owned()) {
            directory = directory.rsplit_once('/').map_or("", |(up, _)| up);
        }
    }
    directories
}

#[must_use]
pub fn digest(entries: &[Entry]) -> String {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort();
    let mut bytes = Vec::new();
    for entry in sorted {
        bytes.extend_from_slice(entry.name.as_encoded_bytes());
        bytes.push(0);
        bytes.push(entry.kind as u8);
    }
    velnor_rust::sha256_hex(&bytes)
}

fn header(inputs_text: &str) -> String {
    format!(
        "{VERSION}\t{}",
        velnor_rust::sha256_hex(inputs_text.as_bytes())
    )
}

#[must_use]
pub fn encode(inputs_text: &str, listings: &Listings) -> String {
    let mut text = header(inputs_text);
    text.push('\n');
    text.extend(
        listings
            .iter()
            .filter(|(directory, _)| is_representable(directory))
            .map(|(directory, entries)| format!("{directory}\t{}\n", digest(entries))),
    );
    text
}

#[must_use]
pub fn decode(text: &str, inputs_text: &str) -> BTreeMap<String, String> {
    let mut lines = text.lines();
    if lines.next() != Some(header(inputs_text).as_str()) {
        return BTreeMap::new();
    }
    lines
        .map(|line| {
            line.split_once('\t')
                .map(|(directory, digest)| (directory.to_owned(), digest.to_owned()))
        })
        .collect::<Option<_>>()
        .unwrap_or_default()
}

#[must_use]
pub fn candidates(recorded: &BTreeMap<String, String>, restored: &[SavedInput]) -> Vec<String> {
    directories(restored)
        .into_iter()
        .filter(|directory| recorded.contains_key(directory))
        .collect()
}

#[must_use]
pub fn plan_times(
    recorded: &BTreeMap<String, String>,
    live: &Listings,
    restored: &[SavedInput],
) -> BTreeMap<String, u64> {
    let files: HashMap<&str, u64> = restored
        .iter()
        .map(|input| (input.file.path.as_str(), input.mtime_ns))
        .collect();
    let mut order: Vec<(&String, &Vec<Entry>)> = live.iter().collect();
    order.sort_by_key(|(directory, _)| std::cmp::Reverse(directory.matches('/').count()));
    let mut times = BTreeMap::new();
    for (directory, entries) in order {
        if entries.is_empty() || recorded.get(directory) != Some(&digest(entries)) {
            continue;
        }
        let newest = entries.iter().try_fold(0, |newest: u64, entry| {
            let path = format!("{directory}/{}", entry.name.to_str()?);
            let time = match entry.kind {
                EntryKind::File => files.get(path.as_str()).copied(),
                EntryKind::Directory => times.get(&path).copied(),
                EntryKind::Other => None,
            };
            time.map(|time| newest.max(time))
        });
        if let Some(newest) = newest {
            times.insert(directory.clone(), newest);
        }
    }
    times
}
