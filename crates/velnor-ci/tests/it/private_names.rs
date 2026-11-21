// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const SALT: &str = "velnor-private-name-guard-v1:";
const HASHES: &str = include_str!("private_names_hashes.txt");
const EXEMPT: [&str; 1] = ["docs/hard-decisions.md"];
const DIGITS: &[u8; 16] = b"0123456789abcdef";
const MAX_RUN: usize = 6;
const MAX_REPORTED: usize = 50;

#[derive(Default)]
struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut state = if self.0 == 0 {
            0xcbf2_9ce4_8422_2325
        } else {
            self.0
        };
        for byte in bytes {
            state = (state ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = state;
    }
}

type Memo = HashMap<String, bool, BuildHasherDefault<Fnv>>;

fn digest(word: &str) -> String {
    let hash = Sha256::new()
        .chain_update(SALT)
        .chain_update(word)
        .finalize();
    let mut hex = String::with_capacity(64);
    for byte in hash {
        hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    hex
}

fn committed() -> HashSet<String> {
    HASHES.lines().map(str::to_owned).collect()
}

const fn word_table() -> [bool; 256] {
    let mut table = [false; 256];
    let mut value: u8 = 0;
    loop {
        table[value as usize] =
            value.is_ascii_alphanumeric() || value == b'_' || value == b'.' || value == b'-';
        if value == u8::MAX {
            return table;
        }
        value += 1;
    }
}

static WORD_BYTES: [bool; 256] = word_table();

fn trim(word: &str) -> &str {
    word.trim_matches(['.', '-', '_'])
}

fn runs(word: &str) -> Vec<&str> {
    let bytes = word.as_bytes();
    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (index, byte) in bytes.iter().enumerate() {
        match (byte.is_ascii_alphanumeric(), start) {
            (true, None) => start = Some(index),
            (false, Some(from)) => {
                parts.push((from, index));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        parts.push((from, bytes.len()));
    }
    let mut found = vec![word];
    for first in 0..parts.len() {
        for last in first..parts.len().min(first + MAX_RUN) {
            found.push(&word[parts[first].0..parts[last].1]);
        }
    }
    found
}

struct Matcher<'a> {
    hashes: &'a HashSet<String>,
    words: Memo,
    pairs: Memo,
}

impl<'a> Matcher<'a> {
    fn new(hashes: &'a HashSet<String>) -> Self {
        Self {
            hashes,
            words: Memo::default(),
            pairs: Memo::default(),
        }
    }

    fn word(&mut self, word: &str) -> bool {
        if let Some(known) = self.words.get(word) {
            return *known;
        }
        let known = runs(word)
            .into_iter()
            .any(|run| self.hashes.contains(&digest(run)));
        self.words.insert(word.to_owned(), known);
        known
    }

    fn pair(&mut self, first: &str, second: &str) -> bool {
        let pair = format!("{}/{}", trim(first), trim(second));
        if let Some(known) = self.pairs.get(&pair) {
            return *known;
        }
        let known = self.hashes.contains(&digest(&pair));
        self.pairs.insert(pair, known);
        known
    }

    fn flagged_lines(&mut self, text: &str) -> Vec<usize> {
        let lowered = text.to_ascii_lowercase();
        let bytes = lowered.as_bytes();
        let mut flagged = Vec::new();
        let mut line = 1;
        let mut previous: Option<(usize, usize)> = None;
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'\n' {
                line += 1;
                previous = None;
                index += 1;
            } else if WORD_BYTES[usize::from(bytes[index])] {
                let start = index;
                while index < bytes.len() && WORD_BYTES[usize::from(bytes[index])] {
                    index += 1;
                }
                let token = &lowered[start..index];
                let slashed = previous
                    .filter(|(_, end)| end + 1 == start && bytes[*end] == b'/')
                    .map(|(from, end)| &lowered[from..end]);
                let hit = self.word(token) || slashed.is_some_and(|first| self.pair(first, token));
                if hit && flagged.last() != Some(&line) {
                    flagged.push(line);
                }
                previous = Some((start, index));
            } else {
                index += 1;
            }
        }
        flagged
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tracked_files(root: &Path) -> BTreeMap<String, Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-s", "-z"])
        .output()
        .unwrap_or_else(|error| unreachable!("git did not start: {error}"));
    assert!(output.status.success(), "git ls-files failed");
    let mut by_object: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in String::from_utf8_lossy(&output.stdout).split('\0') {
        let Some((meta, path)) = entry.split_once('\t') else {
            continue;
        };
        let Some(object) = meta.split(' ').nth(1) else {
            continue;
        };
        by_object
            .entry(object.to_owned())
            .or_default()
            .push(path.to_owned());
    }
    by_object
}

fn text_of(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[test]
fn the_committed_hashes_are_one_salted_digest_per_line() {
    let lines: Vec<&str> = HASHES.lines().collect();
    assert_ne!(lines.len(), 0);
    for line in &lines {
        assert!(
            line.len() == 64 && line.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
            "a hash line is not 64 lowercase hex digits"
        );
    }
    assert_eq!(committed().len(), lines.len());
}

#[test]
fn no_tracked_text_file_contains_a_private_downstream_name() {
    let root = repository_root();
    let hashes = committed();
    let mut matcher = Matcher::new(&hashes);
    let mut found = Vec::new();
    for paths in tracked_files(&root).values() {
        let Some(scannable) = paths.iter().find(|path| !EXEMPT.contains(&path.as_str())) else {
            continue;
        };
        let Some(text) = text_of(&root.join(scannable)) else {
            continue;
        };
        let lines = matcher.flagged_lines(&text);
        for path in paths.iter().filter(|path| !EXEMPT.contains(&path.as_str())) {
            found.extend(lines.iter().map(|line| format!("{path}:{line}")));
        }
    }
    found.sort();
    assert!(
        found.is_empty(),
        "{} line(s) in tracked files contain a private downstream name; use example-org and example-repo placeholders:\n{}",
        found.len(),
        found
            .iter()
            .take(MAX_REPORTED)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn planted(words: &[&str]) -> HashSet<String> {
    words.iter().map(|word| digest(word)).collect()
}

#[test]
fn a_planted_word_is_found_whole_inside_a_longer_name_and_in_a_pair() {
    let hashes = planted(&["zzplanted", "zzorg/zzrepo", "zz-two-parts"]);
    let mut matcher = Matcher::new(&hashes);
    for (line, expected) in [
        ("see ZZPlanted here", true),
        ("see zzplanted. next", true),
        ("rust-zzplanted-server", true),
        ("a_zzplanted_b", true),
        ("https://example.org/zzorg/zzrepo/pull/1", true),
        ("[zz-two-parts](x)", true),
        ("backend-zz-two-parts-x", true),
        ("zzorg / zzrepo", false),
        ("zzplantedx", false),
        ("zzrepo/zzorg", false),
        ("nothing to see", false),
    ] {
        assert_eq!(matcher.flagged_lines(line).len() == 1, expected, "{line}");
    }
}

#[test]
fn the_report_names_the_line_and_never_the_word() {
    let hashes = planted(&["zzplanted"]);
    let mut matcher = Matcher::new(&hashes);
    let lines = matcher.flagged_lines("one\ntwo zzplanted\nthree\nzzplanted again\n");
    assert_eq!(lines, [2, 4]);
}
