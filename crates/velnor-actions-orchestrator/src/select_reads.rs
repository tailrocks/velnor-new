use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;

use crate::discover::Discovery;
use crate::git_paths::split_nul_paths;
use crate::select_affected::Ownership;
use crate::select_reads_scan::{ReadTarget, SourceRead, scan_source};

pub(crate) type Forced = BTreeMap<String, String>;

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

pub(crate) fn forced_warnings(ownership: &Ownership<'_>, forced: &Forced) -> Vec<String> {
    let mut warnings: Vec<String> = forced
        .iter()
        .map(|(id, reason)| format!("undeclared_reads:{}:{reason}", ownership.name_of(id)))
        .collect();
    warnings.sort();
    warnings
}

pub(crate) fn forced_ids(forced: &Forced) -> BTreeSet<String> {
    forced.keys().cloned().collect()
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

fn reason(read: &SourceRead) -> String {
    match &read.target {
        ReadTarget::Path(path) => format!("{}:reads:{path}", read.file),
        ReadTarget::Outside => format!("{}:outside", read.file),
        ReadTarget::Unbounded(token) => format!("{}:{token}", read.file),
    }
}

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
    let tracked: BTreeSet<String> = split_nul_paths(&output.stdout)?;
    let mut by_package: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for file in tracked.iter().filter(|file| is_rust_source(file)) {
        if let Some((dir, id)) = ownership.package_of(file) {
            by_package
                .entry((id.clone(), dir.clone()))
                .or_default()
                .push(file.clone());
        }
    }
    let mut reads = Vec::new();
    for ((id, dir), owned) in by_package {
        for read in package_reads(root, &tracked, &dir, owned) {
            reads.push((id.clone(), read));
        }
    }
    Ok(reads)
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
    reads.sort_by(|left, right| left.file.cmp(&right.file));
    reads
}

fn is_rust_source(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}
