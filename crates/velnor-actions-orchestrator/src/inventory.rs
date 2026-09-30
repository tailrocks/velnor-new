//! Cargo inventory fetching plus locked/offline qualification.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use velnor_actions_mise::{MetadataDiscovery, MetadataQualification, ToolCatalog};
use velnor_actions_rust::{CandidateOutcome, CargoCandidate, WorkspaceRecord, parse_metadata_json};

use crate::OrchestratorError;
use crate::decisions::{MetadataFailure, classify_metadata_failure};
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};

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
    run_with(root, candidates, true, &|manifest| {
        fetch_inventory(root, manifest, &catalog)
    })
}

/// Inventory loop over an immutable snapshot; `reuse=false` is the legacy path.
/// # Errors
/// Returns `preparation_incomplete` when a fetch fails incompletely.
fn run_with(
    root: &Path,
    candidates: &[CargoCandidate],
    reuse: bool,
    load: &dyn Fn(&str) -> Result<WorkspaceRecord, FetchFailure>,
) -> Result<Inventories, OrchestratorError> {
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
    Ok((outcomes, inventories))
}

/// Manifest-to-slot index; each record indexed once, lookups `O(log n)`.
#[derive(Debug, Default)]
struct MemberIndex {
    slots: BTreeMap<String, usize>,
}

fn ok_outcome(manifest: String) -> CandidateOutcome {
    CandidateOutcome {
        manifest,
        metadata_ok: true,
        diagnostic: None,
    }
}

impl MemberIndex {
    /// Index one fetched record's in-workspace member manifests.
    fn insert(&mut self, slot: usize, record: &WorkspaceRecord) {
        for package in &record.packages {
            if package.in_workspace && !package.external {
                self.slots.entry(package.manifest.clone()).or_insert(slot);
            }
        }
    }

    /// Reuse a validated member record; `None` fetches fresh. Never reuses a
    /// manifest declaring its own `[workspace]` root.
    fn reuse_for(
        &self,
        root: &Path,
        manifest: &str,
        inventories: &[(String, WorkspaceRecord)],
    ) -> Option<WorkspaceRecord> {
        let slot = *self.slots.get(manifest)?;
        if declares_workspace_root(&root.join(manifest)) {
            return None;
        }
        inventories.get(slot).map(|(_, record)| record.clone())
    }
}

/// True when the manifest parses with a top-level `[workspace]` table.
/// Unreadable files read as empty: the fresh fetch reports the real error.
fn declares_workspace_root(path: &Path) -> bool {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    toml::from_str::<toml::Table>(&text).is_ok_and(|table| table.contains_key("workspace"))
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
fn fetch_inventory(
    root: &Path,
    manifest: &str,
    catalog: &ToolCatalog,
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
    parse_metadata_json(&json, root, manifest)
        .map_err(|err| FetchFailure::Malformed(err.to_string()))
}

/// True when stderr reports a missing tool rather than a bad manifest.
fn is_tool_missing(stderr: &str) -> bool {
    let text = stderr.to_lowercase();
    [
        "not installed",
        "missing tool",
        "tool missing",
        "no such tool",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

/// Qualify locked/offline resolution wherever a lockfile pins deps.
/// Lockless workspaces have nothing pinned, so nothing to qualify.
/// # Errors
/// Returns `preparation_incomplete` when a lockfile cannot be qualified.
pub(crate) fn qualify_workspaces(
    root: &Path,
    workspaces: &[PlannedWorkspace],
) -> Result<(), OrchestratorError> {
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
            let first = err.to_string();
            let short: String = first
                .lines()
                .next()
                .unwrap_or("metadata_offline")
                .chars()
                .take(160)
                .collect();
            return Err(OrchestratorError::PreparationIncomplete {
                manifest,
                problem: format!("metadata_offline:{short}"),
            });
        }
    }
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
        assert!(!is_tool_missing(
            "error: failed to parse manifest at Cargo.toml"
        ));
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
        }
    }

    fn candidates(manifests: &[String]) -> Vec<CargoCandidate> {
        manifests
            .iter()
            .map(|m| CargoCandidate {
                manifest: m.clone(),
            })
            .collect()
    }

    fn fixture_dir(files: &[(&str, &str)]) -> Result<tempfile::TempDir, String> {
        let dir = tempfile::TempDir::new().map_err(|err| err.to_string())?;
        for (path, body) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            }
            std::fs::write(&full, body).map_err(|err| err.to_string())?;
        }
        Ok(dir)
    }

    const MEMBER: &str = "[package]\nname = \"x\"\nversion = \"0.1.0\"\n";
    const NESTED_ROOT: &str = "[package]\nname = \"n\"\nversion = \"0.1.0\"\n[workspace]\n";

    /// Reuse matches legacy outcomes/inventories; fetches drop 6 to 4.
    /// Each stub fetch stands for one `cargo metadata` subprocess. The outer
    /// record lists the nested path, but its file declares `[workspace]`, so
    /// the nested guard forces a fresh fetch on the reuse path too.
    #[test]
    fn reuse_matches_legacy_outcomes_and_inventories() -> Result<(), String> {
        let head = ["Cargo.toml", "crates/a/Cargo.toml", "crates/b/Cargo.toml"];
        let tail = ["nested/Cargo.toml", "other/Cargo.toml", "bad/Cargo.toml"];
        let names: Vec<&str> = head.into_iter().chain(tail).collect();
        let files: Vec<(&str, &str)> = names.iter().map(|n| (*n, MEMBER)).collect();
        let dir = fixture_dir(&files)?;
        let nested_path = dir.path().join(names[3]);
        std::fs::write(&nested_path, NESTED_ROOT).map_err(|err| err.to_string())?;
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
        let bad = reused
            .0
            .iter()
            .any(|o| o.manifest == "bad/Cargo.toml" && !o.metadata_ok);
        assert!(bad, "malformed outcome preserved");
        let clean = reused.0.iter().all(|o| {
            !o.diagnostic
                .as_deref()
                .unwrap_or_default()
                .starts_with("unexpected")
        });
        assert!(clean, "no unexpected load happened");
        assert_eq!(
            reused.1[3].1.workspace_root, "nested",
            "nested loaded fresh"
        );
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
