//! Nextest sharding proofs, budgets, and exact-base baseline lookup.
//!
//! Shard proofs bind each partition to its plan obligation; the lookup finds
//! the exact-base successful run through pinned `gh`. Cargo-test profiles
//! never shard and never receive archives.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{canonical_json_bytes, digest_b3, validate_digest};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::decisions::select_exact_base_run;
use crate::merge::BaselineManifest;

/// V1 retry budget: retries are always zero.
pub(crate) const MAX_RETRIES: u32 = 0;

/// One selected test: package, target, features, binary, name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct TestIdentity {
    /// Cargo package ID.
    pub(crate) package: String,
    /// Cargo target name.
    pub(crate) target: String,
    /// Sorted enabled features.
    pub(crate) features: Vec<String>,
    /// Test binary name.
    pub(crate) binary: String,
    /// Test name.
    pub(crate) name: String,
}

impl TestIdentity {
    /// Validate fields: nonempty, relative, sorted features.
    /// # Errors
    pub(crate) fn validate(&self) -> Result<(), String> {
        let fields = [&self.package, &self.target, &self.binary, &self.name];
        let clean = fields
            .iter()
            .all(|v| !v.trim().is_empty() && !v.starts_with('/'));
        let sorted = self.features.windows(2).all(|pair| pair[0] <= pair[1]);
        if clean && sorted {
            Ok(())
        } else {
            Err("malformed_test_identity".into())
        }
    }
}

/// Digest over the canonical sorted test inventory.
#[must_use]
pub(crate) fn inventory_digest(tests: &[TestIdentity]) -> String {
    let mut sorted = tests.to_vec();
    sorted.sort();
    canonical_json_bytes(&sorted)
        .map(|bytes| digest_b3(&bytes))
        .unwrap_or_default()
}

/// Merge-time shard proof binding one partition to its plan obligation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ShardProof {
    /// Full shard task ID with its suffix.
    pub(crate) task_id: String,
    /// Plan obligation input digest binding the selected inventory.
    pub(crate) input_digest: String,
    /// Detected test runner.
    pub(crate) runner: String,
    /// One-based shard index.
    pub(crate) shard_index: u32,
    /// Total shard count.
    pub(crate) shard_count: u32,
    /// Sorted tests assigned to this shard.
    pub(crate) tests: Vec<TestIdentity>,
    /// Digest over the canonical sorted full inventory.
    pub(crate) inventory_digest: String,
    /// Archive digest (Nextest only).
    pub(crate) archive_digest: Option<String>,
    /// Metadata proves the target carries no applicable tests.
    pub(crate) no_test_targets: bool,
}

/// Validate merge proofs for the sharded bases of one entry.
/// # Errors
pub(crate) fn check_entry_shards(
    bases: &BTreeSet<String>,
    empty_count: u32,
    proofs: &[ShardProof],
    obligations: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut empty = 0u32;
    for base in bases {
        let group: Vec<&ShardProof> = proofs
            .iter()
            .filter(|p| p.task_id.split("/shard-").next() == Some(base.as_str()))
            .collect();
        empty += check_group(&group, obligations)?;
    }
    if empty == empty_count {
        Ok(())
    } else {
        Err("empty_partition_unproven".into())
    }
}

/// Validate one base group, returning its empty-partition proof count.
fn check_group(
    group: &[&ShardProof],
    obligations: &BTreeMap<String, String>,
) -> Result<u32, String> {
    let Some(first) = group.first() else {
        return Err("missing_shard".into());
    };
    for proof in group {
        if obligations.get(&proof.task_id) != Some(&proof.input_digest) {
            return Err("shard_input_mismatch".into());
        }
        if validate_digest(&proof.input_digest).is_err() {
            return Err("shard_input_mismatch".into());
        }
    }
    if first.runner != "cargo_nextest" {
        return Err("sharding_requires_nextest".into());
    }
    if velnor_actions_mise::requires_archive_transfer(first.shard_count)
        && first.archive_digest.is_none()
        && !first.no_test_targets
    {
        return Err("missing_archive".into());
    }
    for proof in group {
        let same = proof.runner == first.runner && proof.shard_count == first.shard_count;
        let same = same && proof.inventory_digest == first.inventory_digest;
        let same = same && proof.archive_digest == first.archive_digest;
        if !same {
            return Err("shard_group_mismatch".into());
        }
        for test in &proof.tests {
            test.validate()?;
        }
    }
    let mut indices: Vec<u32> = group.iter().map(|proof| proof.shard_index).collect();
    indices.sort_unstable();
    if indices != (1..=first.shard_count).collect::<Vec<_>>() {
        return Err("missing_shard".into());
    }
    let mut union: Vec<TestIdentity> = group.iter().flat_map(|p| p.tests.clone()).collect();
    union.sort();
    if union.len() != union.iter().collect::<BTreeSet<_>>().len() {
        return Err("duplicate_test".into());
    }
    if inventory_digest(&union) != first.inventory_digest {
        return Err("tampered_manifest".into());
    }
    if union.is_empty() && !first.no_test_targets {
        return Err("empty_inventory_unproven".into());
    }
    if union.is_empty() {
        return Ok(0);
    }
    let empty = group.iter().filter(|proof| proof.tests.is_empty()).count();
    Ok(u32::try_from(empty).unwrap_or(u32::MAX))
}

/// Configured resource limits revalidated at merge time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResourceLimits {
    /// Compiler process budget.
    pub(crate) compiler_budget: u32,
    /// Test process budget.
    pub(crate) test_budget: u32,
    /// Matrix `max-parallel` setting.
    pub(crate) max_parallel: u32,
    /// Known runner capacity.
    pub(crate) capacity: u32,
    /// Total requested shards.
    pub(crate) shards: u32,
    /// Configured retries (V1: zero).
    pub(crate) retries: u32,
    /// Shared-service resource groups, when measured (PAR-8.19).
    #[serde(default)]
    pub(crate) resource_groups: Vec<String>,
}

/// Reject zero budgets, over-budget shards, and above-capacity concurrency.
/// # Errors
pub(crate) fn validate_budgets(limits: &ResourceLimits) -> Result<(), String> {
    let budgets = [
        limits.compiler_budget,
        limits.test_budget,
        limits.max_parallel,
    ];
    if budgets.contains(&0) {
        return Err("budget_must_be_positive".into());
    }
    if limits.retries != MAX_RETRIES {
        return Err("retries_disabled".into());
    }
    if limits.shards > limits.test_budget {
        return Err("shards_exceed_test_budget".into());
    }
    if limits.max_parallel > limits.capacity {
        return Err("concurrency_above_capacity".into());
    }
    let mut groups = limits.resource_groups.clone();
    groups.sort();
    groups.dedup();
    if groups != limits.resource_groups || groups.iter().any(|group| group.trim().is_empty()) {
        return Err("resource_groups_unordered".into());
    }
    Ok(())
}

/// Exact-base baseline lookup through pinned `gh` (par §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BaselineLookup {
    /// Full 40-hex base commit SHA.
    pub(crate) base_sha: String,
    /// Generated workflow path.
    pub(crate) workflow: String,
    /// Protected default-branch name.
    pub(crate) branch: String,
}

impl BaselineLookup {
    /// Build a lookup; rejects short SHAs, URLs, wildcards, and shell.
    /// # Errors
    pub(crate) fn new(base: &str, workflow: &str, branch: &str) -> Result<Self, String> {
        if base.len() != 40 || !base.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("base_must_be_full_sha".into());
        }
        for value in [workflow, branch] {
            let spam = ["://", "*", "$", ";", " "]
                .iter()
                .any(|t| value.contains(t));
            if value.trim().is_empty() || spam {
                return Err("bad_lookup_input".into());
            }
        }
        Ok(Self {
            base_sha: base.into(),
            workflow: workflow.into(),
            branch: branch.into(),
        })
    }

    /// Fixed `gh run list` args for the exact workflow and branch.
    #[must_use]
    pub(crate) fn list_args(&self) -> Vec<OsString> {
        let fields = "databaseId,headSha,event,conclusion,headBranch";
        let workflow = self.workflow.as_str();
        let branch = self.branch.as_str();
        [
            "run",
            "list",
            "--workflow",
            workflow,
            "--branch",
            branch,
            "--json",
            fields,
            "--limit",
            "50",
        ]
        .iter()
        .map(OsString::from)
        .collect()
    }

    /// Fixed `gh run download` args for one exact run into fresh temp.
    #[must_use]
    pub(crate) fn download_args(run_id: u64, dir: &Path) -> Vec<OsString> {
        let id = run_id.to_string();
        [
            OsString::from("run"),
            OsString::from("download"),
            OsString::from(id),
        ]
        .into_iter()
        .chain([OsString::from("--dir"), dir.as_os_str().to_owned()])
        .collect()
    }

    /// Run fixed `gh` args under the pinned catalog in `root`.
    fn run(catalog: &ToolCatalog, root: &Path, args: Vec<OsString>) -> Result<String, String> {
        let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args);
        let exec = exec.map_err(|err| err.to_string())?;
        let output = exec
            .command(catalog)
            .map_err(|err| err.to_string())?
            .with_cwd(PathBuf::from(root))
            .run()
            .map_err(|_| "baseline_unavailable".to_owned())?;
        if !output.success {
            return Err("baseline_unavailable".to_owned());
        }
        output
            .stdout_text("gh")
            .map_err(|_| "baseline_unavailable".to_owned())
    }
}

/// Resolve exact-base manifests: list, select, download, exact-filter.
///
/// Plan obligations do not carry per-task compatibility yet, so the expected
/// artifact name is unformable; every run artifact downloads to fresh temp
/// and only exact-base validated baselines survive. Temp is always removed.
/// # Errors
pub(crate) fn resolve_manifests(
    catalog: &ToolCatalog,
    root: &Path,
    base: &str,
    workflow: &str,
    branch: &str,
    artifact: Option<&str>,
) -> Result<Vec<BaselineManifest>, String> {
    let lookup = BaselineLookup::new(base, workflow, branch)?;
    let text = BaselineLookup::run(catalog, root, lookup.list_args())?;
    let run_id = select_exact_base_run(&text, base, branch)?;
    let temp = tempfile::tempdir().map_err(|_| "baseline_unavailable".to_owned())?;
    BaselineLookup::run(
        catalog,
        root,
        crate::cover_baseline::baseline_download_args(
            base,
            workflow,
            branch,
            artifact,
            run_id,
            temp.path(),
        ),
    )?;
    collect_manifests(temp.path(), base)
}

/// Keep temp artifacts matching the exact-base baseline shape.
fn collect_manifests(dir: &Path, base: &str) -> Result<Vec<BaselineManifest>, String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|_| "baseline_unavailable".to_owned())?;
    for entry in entries {
        let entry = entry.map_err(|_| "baseline_unavailable".to_owned())?;
        if let Some(manifest) = crate::cover_baseline::baseline_entry_for(&entry.path(), base) {
            out.push(manifest);
        }
    }
    if out.is_empty() {
        return Err("baseline_unavailable".to_owned());
    }
    out.sort_by(|left, right| left.artifact_name.cmp(&right.artifact_name));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_args_are_fixed_and_validated() {
        let base = "a".repeat(40);
        let lookup =
            BaselineLookup::new(&base, ".github/workflows/velnor.yml", "testmain").expect("valid");
        let list: Vec<String> = lookup
            .list_args()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(list[0..3], ["run", "list", "--workflow"]);
        let dl: Vec<String> = BaselineLookup::download_args(7, std::path::Path::new("/tmp/x"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(dl[0..3], ["run", "download", "7"]);
        assert!(BaselineLookup::new("short", "w", "b").is_err());
        assert!(BaselineLookup::new(&base, "https://evil/x", "b").is_err());
        let other = "b".repeat(40);
        let runs = serde_json::json!([
            {"databaseId": 1, "headSha": other, "headBranch": "t", "event": "push", "conclusion": "success"},
            {"databaseId": 2, "headSha": base, "headBranch": "t", "event": "push", "conclusion": "success"},
        ]);
        assert_eq!(select_exact_base_run(&runs.to_string(), &base, "t"), Ok(2));
        assert!(select_exact_base_run(&runs.to_string(), &"c".repeat(40), "t").is_err());
    }
}
