//! Cargo inventory fetching plus locked/offline qualification.
//! Discovery never resolves (`--no-deps`); only lockful qualification
//! does (`--locked --offline`). Tool snapshots bracket every fetch loop.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use velnor_actions_mise::{MetadataDiscovery, MetadataQualification, ToolCatalog};
use velnor_actions_rust::{CandidateOutcome, CargoCandidate, WorkspaceRecord, parse_metadata_json};

use crate::OrchestratorError;
use crate::decisions::{MetadataFailure, classify_metadata_failure};
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};
use crate::generate::ToolSnapshot;
use crate::inventory_reuse::MemberIndex;

/// Candidate outcomes plus successful manifest inventories.
pub(crate) type Inventories = (Vec<CandidateOutcome>, Vec<(String, WorkspaceRecord)>);

/// Metadata subprocess lanes. One: parallel `cargo metadata` runs contend on
/// Cargo's global package-cache lock; reuse removes redundant subprocesses.
const MAX_METADATA_LANES: usize = 1;

/// Run metadata discovery for every candidate manifest, reusing workspace
/// records after membership validation.
///
/// # Errors
///
/// Returns `preparation_incomplete` when a fetch fails incompletely.
pub(crate) fn run_inventories(
    root: &Path,
    candidates: &[CargoCandidate],
) -> Result<Inventories, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let known: BTreeSet<String> = candidates
        .iter()
        .map(|candidate| candidate.manifest.clone())
        .collect();
    run_with(root, candidates, true, &|manifest| {
        fetch_inventory(root, manifest, &catalog, &known)
    })
}

/// Inventory loop over an immutable snapshot; `reuse=false` is legacy.
/// A tool snapshot brackets the fetches, failing closed on tool drift.
/// # Errors
/// Returns `preparation_incomplete` when a fetch fails incompletely.
fn run_with(
    root: &Path,
    candidates: &[CargoCandidate],
    reuse: bool,
    load: &dyn Fn(&str) -> Result<WorkspaceRecord, FetchFailure>,
) -> Result<Inventories, OrchestratorError> {
    let tools = ToolSnapshot::capture(root);
    let mut outcomes = Vec::with_capacity(candidates.len());
    let mut inventories = Vec::new();
    let mut index = MemberIndex::default();
    for lane in candidates.chunks(MAX_METADATA_LANES) {
        for candidate in lane {
            let manifest = candidate.manifest.clone();
            if reuse && let Some(record) = index.reuse_for(root, &manifest, &inventories) {
                outcomes.push(ok_outcome(manifest.clone()));
                inventories.push((manifest, record));
                continue;
            }
            match load(&manifest) {
                Ok(record) => {
                    outcomes.push(ok_outcome(manifest.clone()));
                    index.insert(inventories.len(), &record);
                    inventories.push((manifest, record));
                }
                Err(FetchFailure::Malformed(diagnostic)) => outcomes.push(CandidateOutcome {
                    manifest,
                    metadata_ok: false,
                    diagnostic: Some(diagnostic),
                }),
                Err(FetchFailure::Incomplete(problem)) => {
                    return Err(OrchestratorError::PreparationIncomplete { manifest, problem });
                }
            }
        }
    }
    tools.verify(root)?;
    Ok((outcomes, inventories))
}

fn ok_outcome(manifest: String) -> CandidateOutcome {
    CandidateOutcome {
        manifest,
        metadata_ok: true,
        diagnostic: None,
    }
}

/// Why one manifest produced no inventory.
enum FetchFailure {
    Malformed(String),
    Incomplete(String),
}

/// Classify a nonzero metadata exit: offline deps abort, malformed selects.
fn classify_exit_failure(stderr: &str) -> FetchFailure {
    match classify_metadata_failure(stderr) {
        MetadataFailure::Incomplete => {
            let first = stderr.lines().next().unwrap_or("metadata_offline").trim();
            let short: String = first.chars().take(160).collect();
            FetchFailure::Incomplete(format!("metadata_offline:{short}"))
        }
        MetadataFailure::Malformed => FetchFailure::Malformed(stderr.to_owned()),
    }
}

/// Discover and parse one manifest through pinned Cargo.
///
/// `known` holds every discovered candidate manifest: path targets naming
/// one are real packages outside this workspace (nested, parent, or
/// sibling members) and skip instead of failing. Unknown targets fail.
fn fetch_inventory(
    root: &Path,
    manifest: &str,
    catalog: &ToolCatalog,
    known: &BTreeSet<String>,
) -> Result<WorkspaceRecord, FetchFailure> {
    let path: PathBuf = root.join(manifest);
    let request = MetadataDiscovery::new(path)
        .map_err(|err| FetchFailure::Incomplete(format!("bad_manifest_path:{err}")))?;
    let command = request
        .command(catalog)
        .map_err(|err| FetchFailure::Incomplete(err.to_string()))?;
    let auto_install_off = command.disables_auto_install();
    let output = command
        .run()
        .map_err(|err| FetchFailure::Incomplete(err.to_string()))?;
    if !output.success {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if auto_install_off && is_tool_missing(&stderr) {
            return Err(FetchFailure::Incomplete(format!(
                "tool_missing:{}",
                stderr.lines().next().unwrap_or_default().trim()
            )));
        }
        return Err(classify_exit_failure(&stderr));
    }
    let json = output
        .stdout_text("mise")
        .map_err(|err| FetchFailure::Incomplete(err.to_string()))?;
    parse_metadata_json(&json, root, manifest, known)
        .map_err(|err| FetchFailure::Malformed(err.to_string()))
}

/// True when stderr reports a missing tool rather than a bad manifest.
fn is_tool_missing(stderr: &str) -> bool {
    let text = stderr.to_lowercase();
    text.contains("not installed")
        || text.contains("missing tool")
        || text.contains("tool missing")
        || text.contains("no such tool")
}

/// Qualify locked/offline resolution where a lockfile pins deps.
/// A tool snapshot brackets the runs, failing closed on tool drift.
/// # Errors
/// Returns `preparation_incomplete` when a lockfile cannot be qualified.
pub(crate) fn qualify_workspaces(
    root: &Path,
    workspaces: &[PlannedWorkspace],
) -> Result<(), OrchestratorError> {
    let tools = ToolSnapshot::capture(root);
    let catalog = ToolCatalog::pinned();
    for workspace in workspaces {
        let prefix = workspace.record.workspace_root.clone();
        let lock = workspace_lock(&prefix);
        if !root.join(&lock).is_file() {
            continue;
        }
        let manifest = workspace_manifest(&prefix);
        let request = MetadataQualification::new(root.join(&manifest)).map_err(|err| {
            OrchestratorError::PreparationIncomplete {
                manifest: manifest.clone(),
                problem: format!("bad_manifest_path:{err}"),
            }
        })?;
        if let Err(err) = request.run(&catalog) {
            let text = err.to_string();
            let first = text.lines().next().unwrap_or("metadata_offline");
            let short: String = first.chars().take(160).collect();
            return Err(OrchestratorError::PreparationIncomplete {
                manifest,
                problem: format!("metadata_offline:{short}"),
            });
        }
    }
    tools.verify(root)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use velnor_actions_rust::PackageRecord;

    #[test]
    fn tool_missing_markers_are_conservative() {
        assert!(is_tool_missing("mise ERROR Tool rust@1.2.3 not installed"));
        assert!(is_tool_missing("No such tool: nextest"));
        assert!(!is_tool_missing("error: bad manifest"));
        assert!(!is_tool_missing(""));
    }

    fn package(manifest: &str) -> PackageRecord {
        PackageRecord {
            id: format!("pkg {manifest}"),
            name: "pkg".to_owned(),
            version: "0.1.0".to_owned(),
            manifest: manifest.to_owned(),
            external: false,
            in_workspace: true,
            targets: Vec::new(),
            features: Vec::new(),
            has_build_script: false,
        }
    }

    fn record(workspace_root: &str, manifests: &[&str]) -> WorkspaceRecord {
        WorkspaceRecord {
            workspace_root: workspace_root.to_owned(),
            members: manifests.iter().map(|m| format!("pkg {m}")).collect(),
            packages: manifests.iter().map(|m| package(m)).collect(),
            edges: Vec::new(),
            skipped_edges: Vec::new(),
        }
    }

    fn candidates(manifests: &[String]) -> Vec<CargoCandidate> {
        let mut out = Vec::with_capacity(manifests.len());
        for manifest in manifests {
            out.push(CargoCandidate {
                manifest: manifest.clone(),
            });
        }
        out
    }

    fn fixture_dir(files: &[(&str, &str)]) -> Result<tempfile::TempDir, String> {
        let dir = tempfile::TempDir::new().map_err(|err| err.to_string())?;
        for (path, body) in files {
            let full = dir.path().join(path);
            let parent = full.parent().ok_or("fixture path lacks a parent")?;
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            std::fs::write(&full, body).map_err(|err| err.to_string())?;
        }
        Ok(dir)
    }

    const MEMBER: &str = "[package]\nname = \"x\"\nversion = \"0.1.0\"\n";
    const NESTED_ROOT: &str = "[package]\nname = \"n\"\nversion = \"0.1.0\"\n[workspace]\n";

    /// Reuse matches legacy outcomes/inventories; fetches drop 6 to 4.
    #[test]
    fn reuse_matches_legacy_outcomes_and_inventories() -> Result<(), String> {
        let head = ["Cargo.toml", "crates/a/Cargo.toml", "crates/b/Cargo.toml"];
        let tail = ["nested/Cargo.toml", "other/Cargo.toml", "bad/Cargo.toml"];
        let names: Vec<&str> = head.into_iter().chain(tail).collect();
        let mut files: Vec<(&str, &str)> = names.iter().map(|n| (*n, MEMBER)).collect();
        files[3] = (names[3], NESTED_ROOT);
        let dir = fixture_dir(&files)?;
        let outer = record("", &[names[0], names[1], names[2], names[3]]);
        let nested = record("nested", &names[3..4]);
        let other = record("other", &names[4..5]);
        let load = |manifest: &str| match manifest {
            "Cargo.toml" | "crates/a/Cargo.toml" | "crates/b/Cargo.toml" => Ok(outer.clone()),
            "nested/Cargo.toml" => Ok(nested.clone()),
            "other/Cargo.toml" => Ok(other.clone()),
            "bad/Cargo.toml" => Err(FetchFailure::Malformed("bad toml".to_owned())),
            unexpected => Err(FetchFailure::Malformed(format!("unexpected:{unexpected}"))),
        };
        let manifests: Vec<String> = names.iter().map(ToString::to_string).collect();
        let run = |reuse: bool| {
            let calls = Cell::new(0_usize);
            let counting = |manifest: &str| {
                calls.set(calls.get() + 1);
                load(manifest)
            };
            let result = run_with(dir.path(), &candidates(&manifests), reuse, &counting);
            result.map(|inventories| (inventories, calls.get()))
        };
        let (legacy, legacy_calls) = run(false).map_err(|err| format!("{err:?}"))?;
        let (reused, reuse_calls) = run(true).map_err(|err| format!("{err:?}"))?;
        assert_eq!(legacy_calls, 6, "legacy loads every candidate");
        assert_eq!(reuse_calls, 4, "members reuse the root record");
        assert_eq!(legacy.0, reused.0, "outcomes match");
        assert_eq!(legacy.1, reused.1, "inventories match");
        let bad = &reused.0[5];
        assert!(!bad.metadata_ok, "malformed preserved");
        assert_eq!(bad.manifest, "bad/Cargo.toml");
        for outcome in &reused.0 {
            if outcome.manifest != "bad/Cargo.toml" {
                assert!(outcome.diagnostic.is_none(), "clean {}", outcome.manifest);
            }
        }
        let nested = &reused.1[3].1;
        assert_eq!(nested.workspace_root, "nested");
        Ok(())
    }

    /// A tool write mid-fetch fails the run instead of slipping through.
    #[test]
    fn tool_mutation_mid_run_fails_closed() -> Result<(), String> {
        let dir = fixture_dir(&[("Cargo.toml", MEMBER), ("mise.toml", "v1")])?;
        let one = candidates(&["Cargo.toml".to_owned()]);
        let loader = |manifest: &str| {
            std::fs::write(dir.path().join("mise.toml"), "v2").expect("mutate tool");
            Ok(record("", &[manifest]))
        };
        match run_with(dir.path(), &one, true, &loader) {
            Err(OrchestratorError::Contract { problem })
                if problem == "tool_files_changed:mise.toml" => {}
            Err(other) => return Err(format!("wrong error: {other:?}")),
            Ok(_) => return Err("expected tool drift failure".to_owned()),
        }
        Ok(())
    }

    /// Incomplete fetches abort both paths with the same error.
    #[test]
    fn incomplete_aborts_both_paths() -> Result<(), String> {
        let dir = fixture_dir(&[("Cargo.toml", MEMBER)])?;
        let manifests = ["Cargo.toml".to_owned()];
        let failed = |_: &str| Err(FetchFailure::Incomplete("metadata_offline:x".to_owned()));
        for reuse in [false, true] {
            let result = run_with(dir.path(), &candidates(&manifests), reuse, &failed);
            match result {
                Err(OrchestratorError::PreparationIncomplete { manifest, problem })
                    if manifest == "Cargo.toml" && problem == "metadata_offline:x" => {}
                Err(other) => return Err(format!("wrong error: {other:?}")),
                Ok(_) => return Err("expected preparation_incomplete".to_owned()),
            }
        }
        Ok(())
    }

    /// Subprocess counts: N+1 legacy, exactly 1 reused; the P13 measurements.
    #[test]
    fn metadata_subprocess_counts() -> Result<(), String> {
        for members in [1_usize, 10, 100] {
            let mut names = vec!["Cargo.toml".to_owned()];
            for index in 0..members {
                names.push(format!("crates/c{index:03}/Cargo.toml"));
            }
            let files: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), MEMBER)).collect();
            let dir = fixture_dir(&files)?;
            let listed: Vec<&str> = names.iter().map(String::as_str).collect();
            let workspace = record("", &listed);
            let count = |reuse: bool| {
                let calls = Cell::new(0_usize);
                let load = |_: &str| {
                    calls.set(calls.get() + 1);
                    Ok(workspace.clone())
                };
                assert!(run_with(dir.path(), &candidates(&names), reuse, &load).is_ok());
                calls.get()
            };
            assert_eq!(count(false), members + 1, "legacy scales with members");
            assert_eq!(count(true), 1, "reuse loads once per workspace");
        }
        Ok(())
    }
}
