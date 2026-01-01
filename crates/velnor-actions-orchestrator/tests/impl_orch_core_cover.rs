//! Covered-plan fixture: every obligation covered plus its manifest.
//!
//! Split from `impl_orch_core` so the core helper file keeps the 400-line
//! gate; merge-shape tests build covered plans through this module.

use velnor_actions_contract::{Plan, canonical_json_bytes, digest_b3};
use velnor_actions_orchestrator::baseline_artifact_numeric_id;

/// Plan JSON with every obligation covered plus its matching manifest.
pub(crate) fn covered_plan(
    plan: &Plan,
) -> Result<(serde_json::Value, serde_json::Value), Box<dyn std::error::Error>> {
    let base = plan.base.clone();
    let base = base.ok_or_else(|| std::io::Error::other("base"))?;
    let compat = digest_b3(b"compat");
    let artifact_name = format!("velnor-baseline-{base}-{compat}");
    let tasks: Vec<serde_json::Value> = plan
        .obligations
        .iter()
        .map(|ob| {
            serde_json::json!({
                "task_id": ob.task_id,
                "task_digest": ob.task_digest,
                "input_digest": ob.input_digest,
                "closure_digest": ob.closure_digest,
                "proof_run_id": 7,
                "observed_run_id": 7,
            })
        })
        .collect();
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    let manifest = serde_json::json!({
        "schema": 2,
        "repository_id": digest_b3(b"repo"),
        "source_commit": base,
        "ref": "refs/heads/testmain",
        "event": "push",
        "workflow_ref": format!("o/r/{workflow}@refs/heads/testmain"),
        "run_id": 7,
        "run_attempt": 1,
        "final_status": "passed",
        "generator_version": plan.generator.version,
        "generator_sha256": plan.generator.sha256,
        "compatibility_id": compat,
        "artifact_id": baseline_artifact_numeric_id(&artifact_name),
        "artifact_name": artifact_name,
        "tasks": tasks,
    });
    let manifest_digest = digest_b3(&canonical_json_bytes(&manifest)?);
    let mut plan_json = serde_json::to_value(plan)?;
    let obligations = plan_json["obligations"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("obligations shape"))?;
    for obligation in obligations {
        obligation["decision"] = serde_json::json!("covered_by_trusted_baseline");
        obligation["baseline_proof"] = serde_json::json!({
            "source_commit": base,
            "run_id": 7,
            "artifact_id": baseline_artifact_numeric_id(&artifact_name),
            "artifact_name": artifact_name,
            "manifest_digest": manifest_digest,
        });
    }
    plan_json["matrix"]["include"] = serde_json::json!([]);
    Ok((plan_json, manifest))
}
