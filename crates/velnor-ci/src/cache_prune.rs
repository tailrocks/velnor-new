// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! What a build directory keeps when it is saved as a base: the units this run used.
//! A path crate's library unit stays. Dropping it makes the next warm run rebuild that crate,
//! and the Cache check reports the rebuild as waste. Test executables, binaries, documentation
//! and units this run did not use still go.

use std::collections::BTreeSet;
use std::path::Path;

use crate::cargo_package::PackageIdentity;
use crate::telemetry::Invocation;

const HASH_LENGTH: usize = 16;

/// Whether `text` is a unit hash on its own: the name of a unit directory in the layout of
/// Cargo 1.100, `build/<package>/<hash>`.
#[must_use]
pub fn is_hash(text: &str) -> bool {
    text.len() == HASH_LENGTH && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The hash cargo puts after a `-` in the name of a unit's file or directory, such as
/// `libserde-0123456789abcdef.rlib`, `build/serde-0123456789abcdef` or
/// `serde-0123456789abcdef.serde.0123456789abcdef-cgu.0.rcgu.o`: the first 16 lower-case
/// hexadecimal digits that follow a `-` and end the name or are followed by a `.`.
#[must_use]
pub fn unit_hash(name: &str) -> Option<&str> {
    let file = Path::new(name).file_name()?.to_str()?;
    if is_hash(file) {
        return Some(file);
    }
    file.match_indices('-').find_map(|(dash, _)| {
        let rest = &file[dash + 1..];
        let hash = rest.get(..HASH_LENGTH)?;
        let tail = &rest[HASH_LENGTH..];
        (hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            && (tail.is_empty() || tail.starts_with('.')))
        .then_some(hash)
    })
}

/// The unit an artifact file belongs to: the hash in its own name, or else in the name of the
/// directory holding it, which is where cargo puts a build script's `build-script-build`, or in
/// the name of the unit directory above `out` (Cargo 1.100).
#[must_use]
pub fn artifact_hash(file: &str) -> Option<&str> {
    let parent = Path::new(file).parent()?;
    unit_hash(file)
        .or_else(|| unit_hash(parent.to_str()?))
        .or_else(|| unit_hash(parent.parent()?.to_str()?).filter(|_| parent.ends_with("out")))
}

/// Whether `entry` of `deps` is an executable (a test, bench or binary) rather than a library.
#[must_use]
pub fn is_executable(name: &str) -> bool {
    match Path::new(name).extension().and_then(|e| e.to_str()) {
        None | Some("exe" | "pdb" | "dSYM") => true,
        Some(_) => false,
    }
}

/// Dependency units a run compiled, checked or reused, by the hash in their file names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsedUnits {
    hashes: BTreeSet<String>,
}

/// A package id cargo recorded for a unit this run compiled or reused.
///
/// Path crates are included. Their library units have to stay in the saved base.
fn is_recorded(package_id: &str) -> bool {
    PackageIdentity::parse(package_id).is_some()
}

impl UsedUnits {
    /// `None` unless every Cargo invocation of the job was captured completely and at least
    /// one unit was seen: a partial record would drop units the job did use.
    #[must_use]
    pub fn from_invocations(invocations: &[Invocation]) -> Option<Self> {
        if invocations.is_empty()
            || invocations
                .iter()
                .any(|invocation| !invocation.capture_complete || invocation.rejected > 0)
            || invocations
                .iter()
                .all(|invocation| invocation.artifacts.is_empty())
        {
            return None;
        }
        let mut used = Self::default();
        for invocation in invocations {
            for artifact in invocation
                .artifacts
                .iter()
                .filter(|artifact| is_recorded(&artifact.package_id))
            {
                used.insert_all(artifact.filenames.iter().map(String::as_str));
            }
            for script in invocation
                .build_scripts
                .iter()
                .filter(|script| is_recorded(&script.package_id))
            {
                let run = Path::new(&script.out_dir).parent();
                used.insert_all(run.and_then(Path::to_str));
            }
        }
        Some(used)
    }

    fn insert_all<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) {
        for name in names {
            if let Some(hash) = artifact_hash(name) {
                self.hashes.insert(hash.to_owned());
            }
        }
    }

    /// A unit's file or directory that this run did not use. Names without a unit hash are
    /// not units and are never reported unused.
    #[must_use]
    pub fn unused(&self, name: &str) -> bool {
        unit_hash(name).is_some_and(|hash| !self.hashes.contains(hash))
    }
}

/// Bytes a prune removed, by what they were.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Dependencies,
    Fingerprints,
    BuildScripts,
    Executables,
    Documentation,
    Scratch,
}

impl Category {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Dependencies => "unused or workspace artifacts",
            Self::Fingerprints => "fingerprints",
            Self::BuildScripts => "build script outputs",
            Self::Executables => "executables and workspace outputs",
            Self::Documentation => "documentation",
            Self::Scratch => "nextest and temporary files",
        }
    }
}
