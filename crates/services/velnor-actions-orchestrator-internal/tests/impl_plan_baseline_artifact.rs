//! Public plan-artifact behavior for accepted and rejected caller baselines.

use std::path::Path;

use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_contract_workflow::{ObligationDecision, Plan};
use velnor_actions_orchestrator_internal::internal::plan_internal;
use velnor_actions_orchestrator_internal::internal_request::publish_plan_files;

use crate::impl_gates_cover::{entries_for, manifest_for, plan_with_manifest};

fn plan_response(
    root: &Path,
    head: &str,
    manifest: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": head,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
        "baseline_manifest": manifest,
    });
    let response = plan_internal(&request.to_string())?;
    Ok(serde_json::from_str(&response)?)
}

#[test]
fn accepted_manifest_is_preserved_in_public_plan_artifact() -> Result<(), Box<dyn std::error::Error>>
{
    let (repo, seed) = plan_with_manifest(None)?;
    let head = seed.head.clone();
    let manifest = manifest_for(&seed, &head, &entries_for(&seed));
    let response = plan_response(repo.path(), &head, manifest.clone())?;
    assert_eq!(response["baseline_manifest"], manifest);
    let plan: Plan = serde_json::from_value(response["plan"].clone())?;
    assert!(
        plan.obligations
            .iter()
            .all(|obligation| obligation.decision == ObligationDecision::CoveredByTrustedBaseline)
    );

    let output_root = tempfile::tempdir()?;
    let response_json = serde_json::to_string(&response)?;
    let artifact_dir = publish_plan_files(&response_json, output_root.path())?;
    let staged = std::fs::read(artifact_dir.join("baseline.json"))?;
    assert_eq!(staged, canonical_json_bytes(&manifest)?);
    Ok(())
}

#[test]
fn rejected_manifest_is_omitted_from_public_plan_artifact() -> Result<(), Box<dyn std::error::Error>>
{
    let (repo, seed) = plan_with_manifest(None)?;
    let head = seed.head.clone();
    let wrong_base = "b".repeat(40);
    assert_ne!(wrong_base, head);
    let manifest = manifest_for(&seed, &wrong_base, &entries_for(&seed));
    let response = plan_response(repo.path(), &head, manifest)?;
    assert!(response.get("baseline_manifest").is_none());
    let plan: Plan = serde_json::from_value(response["plan"].clone())?;
    assert!(
        plan.obligations
            .iter()
            .all(|obligation| obligation.decision == ObligationDecision::Execute)
    );
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.contains("baseline_miss:wrong_commit"))
    );

    let output_root = tempfile::tempdir()?;
    let response_json = serde_json::to_string(&response)?;
    let artifact_dir = publish_plan_files(&response_json, output_root.path())?;
    assert!(!artifact_dir.join("baseline.json").exists());
    Ok(())
}
