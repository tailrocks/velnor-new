// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::delta_snapshot::{self as snapshot, BASELINE, MANIFEST, Manifest, Snapshot};
use super::delta_tree as tree;
use crate::delta_cli::{DeltaArgs, DeltaCommand};
use crate::error::CiError;
use crate::escape::{command_line, escape};

pub(super) const LIMIT: u64 = 500_000_000;
const MARKER: &str = ".velnor/delta-marker";

pub(super) fn run(command: DeltaCommand) -> Result<String, CiError> {
    let args = match &command {
        DeltaCommand::Prepare(args) | DeltaCommand::Apply(args) | DeltaCommand::Stage(args) => args,
    };
    super::delta_paths::untracked(&args.target_dir, "target")?;
    let base = std::env::var("BASE_CACHE_KEY").unwrap_or_default();
    match command {
        DeltaCommand::Prepare(args) => {
            super::delta_paths::staging(&args.staging_dir)?;
            let hash = velnor_rust::sha256_hex(base.as_bytes());
            let root = std::env::current_dir().map_err(|e| CiError::new(e.to_string()))?;
            let identity = super::delta_source::identity(&root);
            super::append_outputs(&format!(
                "base8={}\nsource_identity={}\n",
                &hash[..8],
                identity.as_deref().unwrap_or_default()
            ))?;
            Ok(format!(
                "delta base: {base}\n{}",
                if identity.is_err() {
                    "::notice::source identity unavailable; saving no delta\n"
                } else {
                    ""
                }
            ))
        }
        DeltaCommand::Apply(args) => apply(&args, &base),
        DeltaCommand::Stage(args) => stage(&args, &base),
    }
}

fn apply(args: &DeltaArgs, base: &str) -> Result<String, CiError> {
    super::delta_paths::staging(&args.staging_dir)?;
    if let Err(error) = safe_path(&args.target_dir) {
        return Ok(command_line(
            "notice",
            &format!(
                "{}; skipping delta reuse",
                cleanup(&args.staging_dir, &error)
            ),
        ));
    }
    let cutoff = snapshot::now_ns()?;
    let recorded = snapshot::recorded_inventory(&args.target_dir);
    let baseline = snapshot::capture(&args.target_dir, base, recorded.as_ref(), cutoff).and_then(
        |(baseline, read)| {
            snapshot::validate_times(&baseline.nodes, cutoff)?;
            Ok((baseline, read))
        },
    );
    let manifest = snapshot::read::<Manifest>(&args.staging_dir.join(MANIFEST));
    let mut report = if let Err(error) = &baseline {
        cold(&args.target_dir, error)?
    } else {
        match manifest {
            Ok(manifest) if !base.is_empty() && manifest.base == base => {
                match tree::apply(&manifest, &args.staging_dir, &args.target_dir, cutoff).and_then(
                    |count| {
                        super::delta_freshness::invalidate(&args.target_dir, &manifest.sources)
                            .map(|invalidated| (count, invalidated))
                    },
                ) {
                    Ok((count, invalidated)) => {
                        format!(
                            "applied {count} delta files, preserving modification times; invalidated {invalidated} source-affected fingerprints\n"
                        )
                    }
                    Err(error) => cold(&args.target_dir, &error)?,
                }
            }
            Err(error) if args.staging_dir.exists() => cold(&args.target_dir, &error)?,
            Ok(_) | Err(_) => "no matching delta; using the base build directory\n".to_owned(),
        }
    };
    tree::remove_node(&args.staging_dir)?;
    snapshot::replace(&args.target_dir.join(MARKER), Some(b""))?;
    if let Ok((baseline, read)) = &baseline {
        report = format!("{report}read {read} of {} base files\n", baseline.files());
    }
    let bytes = baseline
        .and_then(|(baseline, _)| snapshot::encode(&baseline))
        .ok();
    snapshot::replace(&args.target_dir.join(BASELINE), bytes.as_deref())?;
    if bytes.is_none() {
        report.push_str("::notice::base inventory unavailable; this run will save no delta\n");
    }
    Ok(report)
}

fn cold(target: &Path, error: &CiError) -> Result<String, CiError> {
    tree::remove_node(target)?;
    Ok(format!(
        "delta apply failed: {}; removed 1 cached target directories and building cold\n",
        escape(&error.to_string(), false)
    ))
}

fn compiled(args: &DeltaArgs) -> bool {
    let log = std::env::var_os("RUNNER_TEMP")
        .and_then(|dir| fs::read_to_string(Path::new(&dir).join("velnor-run.log")).ok())
        .unwrap_or_default();
    log.lines().any(|line| {
        let cleaned = crate::logscan::strip_ansi(crate::logscan::strip_timestamp(line));
        let mut words = cleaned.split_whitespace();
        matches!(words.next(), Some("Compiling" | "Checking" | "Documenting"))
            && words
                .next()
                .is_some_and(|name| args.packages.iter().any(|package| package == name))
    })
}

fn decline(reason: &str) -> Result<String, CiError> {
    super::append_outputs("save=false\n")?;
    Ok(command_line(
        "notice",
        &format!("{reason}; saving no delta"),
    ))
}

fn stage(args: &DeltaArgs, base: &str) -> Result<String, CiError> {
    super::delta_paths::staging(&args.staging_dir)?;
    if let Err(error) = safe_path(&args.target_dir) {
        return decline(&cleanup(&args.staging_dir, &error));
    }
    if base.is_empty() || !compiled(args) {
        return decline("no base or no compilation");
    }
    let root = args
        .target_dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = crate::inputs::inputs_file(&super::input_scopes::git_prefix(root)?);
    let inputs = match super::input_snapshot::verify(root, &args.target_dir.join(&name)) {
        Ok(inputs) => inputs,
        Err(error) => return decline(&error.to_string()),
    };
    let cutoff = snapshot::now_ns()?;
    let baseline = match snapshot::read::<Snapshot>(&args.target_dir.join(BASELINE)) {
        Ok(snapshot)
            if snapshot.base == base
                && snapshot::validate_times(&snapshot.nodes, cutoff).is_ok()
                && snapshot
                    .nodes
                    .keys()
                    .all(|path| snapshot::validate(path).is_ok()) =>
        {
            snapshot
        }
        _ => return decline("no valid base inventory"),
    };
    let (manifest, read, files) = match select(args, base, &baseline, cutoff, inputs) {
        Ok(selected) => selected,
        Err(error) => return decline(&error.to_string()),
    };
    let bytes = match snapshot::encode(&manifest) {
        Ok(bytes) => bytes,
        Err(error) => return decline(&error.to_string()),
    };
    let size = manifest
        .nodes
        .values()
        .fold(0_u64, |size, node| size.saturating_add(node.size()))
        .saturating_add(bytes.len() as u64);
    if size > LIMIT {
        return decline(&over_limit(size).to_string());
    }
    if let Err(error) = materialize(args, &manifest, &bytes, cutoff) {
        return decline(&cleanup(&args.staging_dir, &error));
    }
    super::append_outputs("save=true\n")?;
    Ok(format!(
        "staged {} cumulative delta files, {size} bytes; read {read} of {files} files\n",
        manifest
            .nodes
            .values()
            .filter(|node| !node.directory())
            .count()
    ))
}

fn over_limit(size: u64) -> CiError {
    CiError::new(format!(
        "delta of {size} bytes exceeds the {LIMIT} byte limit"
    ))
}

fn cleanup(staging: &Path, error: &CiError) -> String {
    match tree::remove_node(staging) {
        Ok(()) => error.to_string(),
        Err(cleanup) => format!("{error}; could not remove partial delta: {cleanup}"),
    }
}

fn materialize(
    args: &DeltaArgs,
    manifest: &Manifest,
    bytes: &[u8],
    cutoff: u64,
) -> Result<(), CiError> {
    tree::remove_node(&args.staging_dir)?;
    let payload = args.staging_dir.join("files");
    fs::create_dir_all(&payload).map_err(|error| failure(&payload, &error))?;
    tree::write_nodes(&args.target_dir, &payload, &manifest.nodes)?;
    tree::validate(manifest, &args.staging_dir, &args.target_dir, cutoff)?;
    snapshot::replace(&args.staging_dir.join(MANIFEST), Some(bytes))
}

fn select(
    args: &DeltaArgs,
    base: &str,
    baseline: &Snapshot,
    cutoff: u64,
    inputs: super::input_snapshot::VerifiedInputs,
) -> Result<(Manifest, usize, usize), CiError> {
    let mut current = BTreeMap::new();
    let mut size = 0_u64;
    let (mut read, mut files) = (0, 0);
    for path in tree::paths(&args.target_dir)? {
        let full = args.target_dir.join(&path);
        let stat = snapshot::stat(&full)?;
        stat.validate_time(cutoff)?;
        let original = baseline.nodes.get(&path);
        let known_changed = original.is_none_or(|node| {
            node.directory() != stat.directory()
                || node.size() != stat.size()
                || node.metadata() != stat.metadata()
        });
        if !stat.directory() && known_changed {
            let minimum = (&path, &stat);
            let overhead = snapshot::encode(&minimum)?.len() as u64;
            let attempted = size.saturating_add(stat.size()).saturating_add(overhead);
            if attempted > LIMIT {
                return Err(over_limit(attempted));
            }
        }
        let changed = snapshot::change_time(&full)?;
        let node = if let Some(node) = baseline.unchanged(&path, &stat, changed) {
            node.clone()
        } else {
            read += usize::from(!stat.directory());
            snapshot::identity(&full)?
        };
        files += usize::from(!stat.directory());
        if path == inputs.name || original != Some(&node) {
            size = size.saturating_add(node.size());
        }
        current.insert(path, node);
    }
    if !matches!(current.get(&inputs.name), Some(snapshot::Node::File { digest, .. }) if *digest == inputs.digest)
    {
        return Err(CiError::new(
            "saved inputs changed after source verification",
        ));
    }
    let removed: Vec<_> = baseline
        .nodes
        .iter()
        .filter(|(path, node)| {
            current
                .get(*path)
                .is_none_or(|current| current.directory() != node.directory())
        })
        .map(|(path, _)| path.clone())
        .collect();
    let mut nodes: BTreeMap<_, _> = current
        .iter()
        .filter(|(path, node)| {
            path.as_path() == inputs.name || baseline.nodes.get(*path) != Some(*node)
        })
        .map(|(path, node)| (path.clone(), node.clone()))
        .collect();
    for path in nodes.keys().chain(&removed).cloned().collect::<Vec<_>>() {
        for parent in path.ancestors().skip(1) {
            if let Some(node) = current.get(parent) {
                nodes.insert(parent.to_path_buf(), node.clone());
            }
        }
    }
    let manifest = Manifest {
        base: base.to_owned(),
        nodes,
        removed,
        sources: inputs.sources,
    };
    Ok((manifest, read, files))
}

pub(super) fn safe_path(path: &Path) -> Result<(), CiError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CiError::new(format!("{} is a symlink", current.display())));
            }
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) => {}
            Err(e) => return Err(failure(&current, &e)),
        }
    }
    Ok(())
}

pub(super) fn failure(path: &Path, error: &std::io::Error) -> CiError {
    CiError::new(format!("{}: {error}", path.display()))
}
