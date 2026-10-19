//! Seed lane message parsing: which compiled units the dependency seed keeps.
//!
//! A unit is kept when Cargo reported it in this run (`compiler-artifact`,
//! fresh or not, or `build-script-executed`) and its package is a registry
//! or Git package that is not path-tainted: its dependency closure in the
//! resolve graph never reaches a path package (a member or a `[patch]`
//! path crate, whose fingerprints are mtime-based and whose code is
//! stubbed). Units are named by Cargo's 16-hex unit hash, shared by the
//! unit's `deps/` files, its `build/<name>-<hash>` directories, and its
//! `.fingerprint/<name>-<hash>` directory.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use super::super::skeleton::Metadata;
use crate::OrchestratorError;

/// Unit hashes kept and dropped by one lane, plus freshness counts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct LaneUnits {
    /// Hashes of kept dependency units.
    pub(crate) kept: BTreeSet<String>,
    /// Hashes of dropped units (members, path, tainted, unknown packages).
    pub(crate) dropped: BTreeSet<String>,
    /// Kept `compiler-artifact` messages Cargo compiled in this run.
    pub(crate) compiled: u64,
    /// Kept `compiler-artifact` messages Cargo found fresh (restored).
    pub(crate) fresh: u64,
    /// JSON messages read.
    pub(crate) messages: u64,
}

/// Package IDs whose units the seed may keep.
pub(crate) fn keepable_packages(metadata: &Metadata) -> BTreeSet<String> {
    let path = metadata.path_packages();
    let mut reverse: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (node, deps) in metadata.edges() {
        for dep in deps {
            reverse.entry(dep).or_default().push(node);
        }
    }
    let mut tainted: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<&str> = path.iter().copied().collect();
    while let Some(id) = stack.pop() {
        for parent in reverse.get(id).into_iter().flatten() {
            if tainted.insert(parent) {
                stack.push(parent);
            }
        }
    }
    metadata
        .packages
        .iter()
        .map(|package| package.id.as_str())
        .filter(|id| !path.contains(id) && !tainted.contains(id))
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Deserialize)]
struct Message {
    reason: String,
    #[serde(default)]
    package_id: Option<String>,
    #[serde(default)]
    filenames: Vec<String>,
    #[serde(default)]
    out_dir: Option<String>,
    #[serde(default)]
    fresh: Option<bool>,
}

/// Read every `*.jsonl` of one lane and classify its units.
///
/// Lines that are not JSON objects (Nextest's own list output) are skipped;
/// only units under `profile` count.
/// # Errors
///
/// Returns an IO error for an unreadable lane directory or file.
pub(crate) fn lane_units(
    lane_dir: &Path,
    profile: &Path,
    keepable: &BTreeSet<String>,
) -> Result<LaneUnits, OrchestratorError> {
    let mut units = LaneUnits::default();
    let io = |path: &Path, err: std::io::Error| {
        OrchestratorError::io(path.display().to_string(), err.to_string())
    };
    let mut files: Vec<_> = std::fs::read_dir(lane_dir)
        .map_err(|err| io(lane_dir, err))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    files.sort();
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|err| io(&file, err))?;
        for line in text.lines().filter(|line| line.starts_with('{')) {
            let Ok(message) = serde_json::from_str::<Message>(line) else {
                continue;
            };
            units.messages += 1;
            record(&mut units, &message, profile, keepable);
        }
    }
    Ok(units)
}

fn record(units: &mut LaneUnits, message: &Message, profile: &Path, keepable: &BTreeSet<String>) {
    let paths: Vec<&str> = match message.reason.as_str() {
        "compiler-artifact" => message.filenames.iter().map(String::as_str).collect(),
        "build-script-executed" => message.out_dir.iter().map(String::as_str).collect(),
        _ => return,
    };
    let keep = message
        .package_id
        .as_ref()
        .is_some_and(|id| keepable.contains(id));
    let mut hashes = BTreeSet::new();
    for path in paths {
        if let Some(hash) = Path::new(path)
            .strip_prefix(profile)
            .ok()
            .and_then(unit_hash_of_relative)
        {
            hashes.insert(hash);
        }
    }
    if hashes.is_empty() {
        return;
    }
    if keep && message.reason == "compiler-artifact" {
        if message.fresh == Some(true) {
            units.fresh += 1;
        } else {
            units.compiled += 1;
        }
    }
    let set = if keep {
        &mut units.kept
    } else {
        &mut units.dropped
    };
    set.extend(hashes);
}

/// Unit hash of a profile-relative output path (`deps/<file>`,
/// `build/<name>-<hash>/...`); other outputs (uplifted copies) carry none.
fn unit_hash_of_relative(relative: &Path) -> Option<String> {
    let mut parts = relative.iter().filter_map(|part| part.to_str());
    match parts.next()? {
        "deps" | "build" => unit_hash(parts.next()?),
        _ => None,
    }
}

/// The 16-hex hash after the last `-` of a file or directory name, when it
/// ends the name or precedes a `.` extension.
pub(crate) fn unit_hash(name: &str) -> Option<String> {
    let mut found = None;
    for (at, _) in name.match_indices('-') {
        let rest = &name[at + 1..];
        let Some(candidate) = rest.get(..16) else {
            continue;
        };
        let boundary = rest.as_bytes().get(16).is_none_or(|byte| *byte == b'.');
        if boundary
            && candidate
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            found = Some(candidate.to_owned());
        }
    }
    found
}
