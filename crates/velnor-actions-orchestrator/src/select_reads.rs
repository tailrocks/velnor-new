//! Fail-safe signals that force a package to count as changed (hard decision 4).
//!
//! Selection narrows a pull request to the packages whose claimed paths
//! changed. That is sound only while every package reads nothing outside
//! the paths it claims. Until in-job read verification exists, two static
//! signals mark a package whose reads are not bounded:
//!
//! - **Build script.** A package with a `build.rs` may read anything
//!   (`build_script`).
//! - **Out-of-directory source reads.** A tracked `.rs` file owned by the
//!   package reads a path the package does not claim, found by
//!   [`scan_source`](crate::select_reads_scan::scan_source). A resolved
//!   read is covered when the package claims the exact target (its own
//!   directory, a scoped config, its workspace root, a declared
//!   `[[stacks.rust.inputs]]` rule, or a tracked symlink), or when no
//!   package claims it at all: a change there is unowned and already
//!   selects everything. An unbounded
//!   read (`CARGO_MANIFEST_DIR` plus `..`, a computed include) is covered only
//!   once a `[[stacks.rust.inputs]]` rule naming the package sets
//!   `complete = true`: the owner's statement that its declared inputs list
//!   every read, which Velnor cannot verify statically. A read outside the repository can never change with a
//!   pull request and is ignored.
//!
//! A forced package seeds the reverse closure like a changed one, so its
//! dependents run as well.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;

use crate::discover::Discovery;
use crate::git_paths::split_nul_paths;
use crate::select_affected::Ownership;
use crate::select_reads_scan::{ReadTarget, SourceRead, scan_source};

/// Forced package IDs, each with its first uncovered reason.
pub(crate) type Forced = BTreeMap<String, String>;

/// Every package whose reads selection cannot bound.
///
/// # Errors
///
/// Returns a problem token when the index cannot be listed; the caller
/// broadens.
pub(crate) fn forced_packages(
    root: &Path,
    discovery: &Discovery,
    ownership: &Ownership<'_>,
) -> Result<Forced, String> {
    let mut forced = Forced::new();
    for task in &discovery.proposals {
        if task.stack_id == velnor_actions_rust::STACK_ID
            && task.identity.undeclared_reads
            && !task.identity.unit_id.is_empty()
        {
            forced
                .entry(task.identity.unit_id.clone())
                .or_insert_with(|| "build_script".to_owned());
        }
    }
    for (id, read) in source_reads(root, ownership)? {
        if !covered(ownership, &id, &read.target) && !forced.contains_key(&id) {
            forced.insert(id, reason(&read));
        }
    }
    Ok(forced)
}

/// Stable `undeclared_reads:<package>:<reason>` warnings, one per package.
pub(crate) fn forced_warnings(ownership: &Ownership<'_>, forced: &Forced) -> Vec<String> {
    let mut warnings: Vec<String> = forced
        .iter()
        .map(|(id, reason)| format!("undeclared_reads:{}:{reason}", ownership.name_of(id)))
        .collect();
    warnings.sort();
    warnings
}

/// IDs of a forced map.
pub(crate) fn forced_ids(forced: &Forced) -> BTreeSet<String> {
    forced.keys().cloned().collect()
}

/// True when package `id` claims what `target` reads.
///
/// An unclaimed target is covered only when nothing claimable lies at or
/// below it: a change to an unowned file already runs everything, but a
/// read of a directory (the repository root, `crates/`) reaches every
/// package beneath it.
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

/// Warning reason for one uncovered read.
fn reason(read: &SourceRead) -> String {
    match &read.target {
        ReadTarget::Path(path) => format!("{}:reads:{path}", read.file),
        ReadTarget::Outside => format!("{}:outside", read.file),
        ReadTarget::Unbounded(token) => format!("{}:{token}", read.file),
    }
}

/// Every read each package's compiled sources perform.
///
/// A package compiles the tracked `.rs` files it owns plus every unowned
/// `.rs` file they pull in through `#[path]` or `include!` (shared test
/// support), transitively; included files resolve `CARGO_MANIFEST_DIR`
/// against the including package. Anchor and escape signals combine over
/// all of a package's files.
fn source_reads(
    root: &Path,
    ownership: &Ownership<'_>,
) -> Result<Vec<(String, SourceRead)>, String> {
    let output = GitRequest::ls_files(vec![OsString::from("-z")])
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    let files: Vec<String> = split_nul_paths(&output.stdout)?;
    let sources: BTreeSet<String> = files
        .into_iter()
        .filter(|file| is_rust_source(file))
        .collect();
    let mut by_package: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for file in &sources {
        if let Some((dir, id)) = ownership.package_of(file) {
            by_package
                .entry((id.clone(), dir.clone()))
                .or_default()
                .push(file.clone());
        }
    }
    let mut reads = Vec::new();
    for ((id, dir), owned) in by_package {
        for read in package_reads(root, ownership, &sources, &dir, owned) {
            reads.push((id.clone(), read));
        }
    }
    Ok(reads)
}

/// Reads of one package: its owned files plus the unowned files they include.
fn package_reads(
    root: &Path,
    ownership: &Ownership<'_>,
    sources: &BTreeSet<String>,
    dir: &str,
    owned: Vec<String>,
) -> Vec<SourceRead> {
    let mut queue = owned;
    let mut seen: BTreeSet<String> = queue.iter().cloned().collect();
    let mut reads = Vec::new();
    let mut anchor: Option<String> = None;
    let mut escapes = false;
    while let Some(file) = queue.pop() {
        // A tracked file missing from the worktree or not UTF-8 cannot
        // compile; the dirty-worktree and Cargo checks own those cases.
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
                && sources.contains(target)
                && ownership.package_of(target).is_none()
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
        });
    }
    reads.sort_by(|left, right| left.file.cmp(&right.file));
    reads
}

/// True for a Rust source path (`.rs`, any case).
fn is_rust_source(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}
