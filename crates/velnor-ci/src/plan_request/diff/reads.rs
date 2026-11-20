// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::claims::Ownership;
use super::scan::{ReadTarget, SourceRead, scan_source};
use super::units::Unit;

pub(super) fn forced_packages(
    root: &Path,
    tracked: &BTreeSet<String>,
    units: &[Unit],
    ownership: &Ownership<'_>,
) -> BTreeSet<String> {
    let mut forced: BTreeSet<String> = units
        .iter()
        .filter(|unit| unit.build_script)
        .map(|unit| unit.dir.clone())
        .collect();
    let mut by_package: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for file in tracked.iter().filter(|file| is_rust_source(file)) {
        if let Some((dir, id)) = ownership.package_of(file) {
            by_package
                .entry((id.clone(), dir.clone()))
                .or_default()
                .push(file.clone());
        }
    }
    for ((id, dir), owned) in by_package {
        if forced.contains(&id) {
            continue;
        }
        if package_reads(root, tracked, &dir, owned)
            .iter()
            .any(|read| !covered(ownership, &id, &read.target))
        {
            forced.insert(id);
        }
    }
    forced
}

fn covered(ownership: &Ownership<'_>, id: &str, target: &ReadTarget) -> bool {
    match target {
        ReadTarget::Path(path) => {
            ownership.claim(path).contains(id)
                || (!ownership.is_owned(path) && !ownership.has_claims_below(path))
        }
        ReadTarget::Outside => true,
        ReadTarget::Unbounded(_) => ownership.declares_complete_inputs(id),
    }
}

fn package_reads(
    root: &Path,
    tracked: &BTreeSet<String>,
    dir: &str,
    owned: Vec<String>,
) -> Vec<SourceRead> {
    let mut queue = owned;
    let mut seen: BTreeSet<String> = queue.iter().cloned().collect();
    let mut reads = Vec::new();
    let mut anchor: Option<String> = None;
    let mut escapes = false;
    while let Some(file) = queue.pop() {
        let Ok(text) = std::fs::read_to_string(root.join(&file)) else {
            continue;
        };
        let scanned = scan_source(&file, dir, &text);
        if scanned.anchored && anchor.is_none() {
            anchor = Some(file.clone());
        }
        escapes |= scanned.escapes;
        for read in scanned.reads {
            if let ReadTarget::Path(target) = &read.target
                && read.compiled
                && tracked.contains(target)
                && seen.insert(target.clone())
            {
                queue.push(target.clone());
            }
            reads.push(read);
        }
    }
    if let (Some(file), true) = (anchor, escapes) {
        reads.push(SourceRead {
            file,
            target: ReadTarget::Unbounded("manifest_dir_escape"),
            compiled: false,
        });
    }
    reads
}

fn is_rust_source(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}
