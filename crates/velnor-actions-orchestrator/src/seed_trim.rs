//! `seed-trim-v1`: trim the lane profiles to one complete dependency seed.
//!
//! Per lane profile directory it keeps `deps/` files, `build/` directories,
//! and `.fingerprint/` directories of the units the lane kept (see
//! [`units`]) and deletes everything else: member, path, and tainted
//! units, executables, `incremental`, `examples`, `.cargo-lock`, symlinks,
//! and units left over from the restored previous generation. It refuses to
//! run while a compiler process is alive, writes `seed-report.json`, and
//! publishes `within_ceiling` / `helper_within_ceiling` for the save gates.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, SEED_HELPER_WITHIN_CEILING_OUTPUT, SEED_WITHIN_CEILING_OUTPUT, SeedLane,
};

use super::skeleton::Metadata;
use crate::OrchestratorError;
use crate::internal::internal;

#[path = "seed_trim_units.rs"]
pub(crate) mod units;

/// Compiler processes that must have exited before the trim.
const COMPILERS: [&str; 6] = [
    "cargo",
    "rustc",
    "mbx",
    "clippy-driver",
    "rustdoc",
    "cargo-nextest",
];

/// One lane's trim result.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct LaneReport {
    pub(crate) ran: bool,
    pub(crate) kept_units: usize,
    pub(crate) dropped_units: usize,
    pub(crate) compiled: u64,
    pub(crate) fresh: u64,
    pub(crate) messages: u64,
    pub(crate) millis: Option<u64>,
}

/// One family's verdict.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct FamilyReport {
    pub(crate) complete: bool,
    pub(crate) raw_bytes: u64,
    pub(crate) within_ceiling: bool,
}

/// Trim inputs read from the runner environment.
pub(crate) struct TrimInputs<'a> {
    pub(crate) workspace: &'a Path,
    pub(crate) seed_dir: &'a Path,
    pub(crate) root: &'a str,
    pub(crate) lanes: &'a [SeedLane],
    pub(crate) ceiling: u64,
}

/// Run the trim from the runner environment; returns the log summary.
/// # Errors
///
/// Returns an internal or IO error for missing context, a live compiler,
/// unreadable lanes, or an unwritable report or output file.
pub fn seed_trim() -> Result<String, OrchestratorError> {
    let workspace = super::absolute_env("GITHUB_WORKSPACE")?;
    let runner_temp = super::absolute_env("RUNNER_TEMP")?;
    let output = super::absolute_env("GITHUB_OUTPUT")?;
    let root = super::seed_root_env()?;
    let lanes = std::env::var("VELNOR_SEED_LANES")
        .map_err(|_| internal("missing_seed_lanes"))?
        .split(',')
        .map(|name| match name {
            "helper" => Some(SeedLane::Helper),
            other => SeedLane::for_task_kind(other),
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| internal("unknown_seed_lane"))?;
    let ceiling = std::env::var("VELNOR_SEED_MAX_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("bad_seed_max_bytes"))?;
    check_no_compiler(Path::new("/proc"))?;
    let seed_dir = runner_temp.join(super::SEED_DIR);
    let inputs = TrimInputs {
        workspace: &workspace,
        seed_dir: &seed_dir,
        root: &root,
        lanes: &lanes,
        ceiling,
    };
    let (lane_reports, families) = trim(&inputs)?;
    let report = serde_json::json!({
        "schema": 1,
        "root": root,
        "key": std::env::var("VELNOR_SEED_KEY").unwrap_or_default(),
        "helper_key": std::env::var("VELNOR_SEED_HELPER_KEY").ok(),
        "ceiling": ceiling,
        "lanes": lane_reports.iter().map(|(lane, report)| (lane.as_str(), report)).collect::<BTreeMap<_, _>>(),
        "families": families.iter().map(|(family, report)| (family.spec().layer, report)).collect::<BTreeMap<_, _>>(),
    });
    let report_path = seed_dir.join("seed-report.json");
    std::fs::write(&report_path, format!("{report}\n"))
        .map_err(|err| OrchestratorError::io(report_path.display().to_string(), err.to_string()))?;
    publish(&output, &families)?;
    Ok(summary(&lane_reports, &families, ceiling))
}

/// Per-lane and per-family trim results.
pub(crate) type TrimReports = (
    BTreeMap<SeedLane, LaneReport>,
    BTreeMap<CacheFamily, FamilyReport>,
);

/// Trim every listed lane that ran; families complete only when all ran.
/// # Errors
///
/// Returns an internal or IO error for unreadable metadata or lanes.
pub(crate) fn trim(inputs: &TrimInputs<'_>) -> Result<TrimReports, OrchestratorError> {
    let metadata = Metadata::read(&inputs.seed_dir.join("metadata.json"))?;
    let keepable = units::keepable_packages(&metadata);
    let mut reports = BTreeMap::new();
    for lane in inputs.lanes {
        let lane_dir = inputs.seed_dir.join(lane.as_str());
        if !lane_dir.is_dir() {
            reports.insert(*lane, LaneReport::default());
            continue;
        }
        let profile = inputs.workspace.join(lane.profile_dir(inputs.root));
        let found = units::lane_units(&lane_dir, &profile, &keepable)?;
        trim_profile(&profile, &found.kept)?;
        reports.insert(
            *lane,
            LaneReport {
                ran: true,
                kept_units: found.kept.len(),
                dropped_units: found.dropped.difference(&found.kept).count(),
                compiled: found.compiled,
                fresh: found.fresh,
                messages: found.messages,
                millis: lane_millis(&lane_dir),
            },
        );
    }
    let mut families = BTreeMap::new();
    for family in [CacheFamily::Rsdeps, CacheFamily::RsdepsHelper] {
        let lanes: Vec<_> = inputs
            .lanes
            .iter()
            .filter(|lane| lane.family() == family)
            .collect();
        if lanes.is_empty() {
            continue;
        }
        let complete = lanes
            .iter()
            .all(|lane| reports.get(lane).is_some_and(|report| report.ran));
        let mut raw_bytes = 0;
        for lane in &lanes {
            raw_bytes += tree_bytes(&inputs.workspace.join(lane.profile_dir(inputs.root)))?;
        }
        families.insert(
            family,
            FamilyReport {
                complete,
                raw_bytes,
                within_ceiling: complete && raw_bytes <= inputs.ceiling,
            },
        );
    }
    Ok((reports, families))
}

/// Keep only the listed units' files in one profile; create it when absent.
fn trim_profile(profile: &Path, kept: &BTreeSet<String>) -> Result<(), OrchestratorError> {
    std::fs::create_dir_all(profile).map_err(|err| io(profile, &err))?;
    for entry in read_dir(profile)? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if matches!(name.as_str(), "deps" | "build" | ".fingerprint") && is_real_dir(&path) {
            for unit in read_dir(&path)? {
                let unit_name = unit.file_name().to_string_lossy().into_owned();
                let keep = units::unit_hash(&unit_name).is_some_and(|hash| kept.contains(&hash));
                if !keep || is_symlink(&unit.path()) {
                    remove(&unit.path())?;
                } else {
                    remove_symlinks(&unit.path())?;
                }
            }
        } else {
            remove(&path)?;
        }
    }
    Ok(())
}

fn remove_symlinks(path: &Path) -> Result<(), OrchestratorError> {
    if !is_real_dir(path) {
        return Ok(());
    }
    for entry in read_dir(path)? {
        let child = entry.path();
        if is_symlink(&child) {
            remove(&child)?;
        } else {
            remove_symlinks(&child)?;
        }
    }
    Ok(())
}

fn tree_bytes(path: &Path) -> Result<u64, OrchestratorError> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(0);
    };
    if !meta.is_dir() {
        return Ok(meta.len());
    }
    let mut total = 0;
    for entry in read_dir(path)? {
        total += tree_bytes(&entry.path())?;
    }
    Ok(total)
}

fn lane_millis(lane_dir: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(lane_dir.join("timing")).ok()?;
    let mut parts = text.split_whitespace().map(str::parse::<u64>);
    let (Some(Ok(start)), Some(Ok(end))) = (parts.next(), parts.next()) else {
        return None;
    };
    end.checked_sub(start)
}

/// Fail while any compiler process named in `/proc/<pid>/comm` is alive.
/// # Errors
///
/// Returns an internal error when `/proc` is unreadable or a compiler runs.
pub(crate) fn check_no_compiler(proc_root: &Path) -> Result<(), OrchestratorError> {
    let entries = std::fs::read_dir(proc_root).map_err(|_| internal("seed_proc_unavailable"))?;
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        if COMPILERS.contains(&comm.trim()) {
            return Err(internal(&format!(
                "seed_compiler_still_running:{}",
                comm.trim()
            )));
        }
    }
    Ok(())
}

fn publish(
    output: &Path,
    families: &BTreeMap<CacheFamily, FamilyReport>,
) -> Result<(), OrchestratorError> {
    let verdict = |family| {
        families
            .get(&family)
            .is_some_and(|report: &FamilyReport| report.within_ceiling)
    };
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(output)
        .map_err(|err| io(output, &err))?;
    writeln!(
        file,
        "{SEED_WITHIN_CEILING_OUTPUT}={}\n{SEED_HELPER_WITHIN_CEILING_OUTPUT}={}",
        verdict(CacheFamily::Rsdeps),
        verdict(CacheFamily::RsdepsHelper)
    )
    .map_err(|err| io(output, &err))
}

fn summary(
    lanes: &BTreeMap<SeedLane, LaneReport>,
    families: &BTreeMap<CacheFamily, FamilyReport>,
    ceiling: u64,
) -> String {
    let mut summary_lines: Vec<String> = lanes
        .iter()
        .map(|(lane, report)| {
            format!(
                "velnor: seed lane {} ran={} kept={} dropped={} compiled={} fresh={}",
                lane.as_str(),
                report.ran,
                report.kept_units,
                report.dropped_units,
                report.compiled,
                report.fresh
            )
        })
        .collect();
    for (family, report) in families {
        let layer = family.spec().layer;
        summary_lines.push(format!(
            "velnor: seed {layer} complete={} raw_bytes={} ceiling={ceiling}",
            report.complete, report.raw_bytes
        ));
        if report.complete && !report.within_ceiling {
            summary_lines.push(format!(
                "::warning::seed: over_ceiling {layer} raw_bytes={} > seed_max_bytes={ceiling}; not saved",
                report.raw_bytes
            ));
        }
    }
    summary_lines.push(String::new());
    summary_lines.join("\n")
}

fn read_dir(path: &Path) -> Result<Vec<std::fs::DirEntry>, OrchestratorError> {
    let mut entries: Vec<_> = std::fs::read_dir(path)
        .map_err(|err| io(path, &err))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    Ok(entries)
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn remove(path: &Path) -> Result<(), OrchestratorError> {
    let result = if is_real_dir(path) {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    result.map_err(|err| io(path, &err))
}

fn io(path: &Path, err: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), err.to_string())
}

#[cfg(test)]
#[path = "seed_trim_tests.rs"]
mod tests;
