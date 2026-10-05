//! F1 end to end: the merge inventory equals the gate's `needs`.
//!
//! `VELNOR_NEEDS_EXPECTED` must list exactly the jobs `toJSON(needs)`
//! can observe at runtime — the `required` job's `needs`. Deriving it
//! from the finalized job set admitted the downstream
//! `publish-baseline` job (which needs `required` and so can never be
//! in `required.needs`), failing the merge with
//! `needs_inventory_mismatch` on every run.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::{TestResult, config_with_branch, make_repo};

/// `needs:` entries per job parsed from one rendered workflow.
fn job_needs(yaml: &str) -> BTreeMap<String, Vec<String>> {
    let mut jobs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut in_jobs = false;
    let mut in_needs = false;
    for line in yaml.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        if let Some(id) = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && rest.ends_with(':') && !rest.contains(' '))
        {
            current = Some(id.trim_end_matches(':').to_owned());
            in_needs = false;
            continue;
        }
        if line == "    needs:" {
            in_needs = true;
            continue;
        }
        if in_needs {
            if let (Some(id), Some(entry)) = (
                current.as_ref(),
                line.strip_prefix("      - ").map(str::trim),
            ) {
                jobs.entry(id.clone()).or_default().push(entry.to_owned());
            } else {
                in_needs = false;
            }
        }
    }
    jobs
}

/// `VELNOR_NEEDS_EXPECTED` payloads parsed from one rendered workflow.
fn expected_inventories(yaml: &str) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let mut inventories = Vec::new();
    for line in yaml
        .lines()
        .filter(|line| line.contains("VELNOR_NEEDS_EXPECTED:"))
    {
        let (_, scalar) = line
            .split_once("VELNOR_NEEDS_EXPECTED:")
            .ok_or("expected scalar")?;
        let quoted = scalar
            .trim()
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .ok_or("expected quoted scalar")?;
        let inventory: Vec<String> = serde_json::from_str(&quoted.replace("\\\"", "\""))?;
        inventories.push(inventory);
    }
    Ok(inventories)
}

#[test]
fn generated_ci_gate_matches_needs_and_excludes_downstream() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    let yaml = fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
    let needs = job_needs(&yaml);
    let gate: BTreeSet<String> = needs
        .get("required")
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    assert!(!gate.is_empty(), "required must need jobs: {needs:?}");
    assert!(
        !gate.contains("publish-baseline"),
        "gate cannot need its downstream: {gate:?}"
    );
    let publish = needs.get("publish-baseline").cloned().unwrap_or_default();
    assert!(
        publish.contains(&"required".to_owned()),
        "publish-baseline stays downstream of required: {publish:?}"
    );
    let expected = expected_inventories(&yaml)?;
    assert_eq!(expected.len(), 2, "write-request plus merge carry it");
    for inventory in &expected {
        let seen: BTreeSet<String> = inventory.iter().cloned().collect();
        assert_eq!(seen, gate, "EXPECTED must equal required.needs");
        assert!(
            !inventory.contains(&"publish-baseline".to_owned()),
            "EXPECTED excludes downstream: {inventory:?}"
        );
    }
    Ok(())
}
