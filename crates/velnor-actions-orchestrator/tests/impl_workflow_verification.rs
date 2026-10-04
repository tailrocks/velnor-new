//! Main CI cadence and manual-dispatch trigger emission.

use velnor_actions_contract::{ObligationDecision, Plan, VerificationScope};
use velnor_actions_orchestrator::{plan_internal, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use crate::impl_common::{TestResult, config_with_branch, git, git_line, make_repo};

const VERIFICATION_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[workflow.verification]\nschedule = \"17 3 * * *\"\nworkflow_dispatch = true\n";

#[test]
fn configured_verification_triggers_emit_schedule_and_dispatch_inputs() -> TestResult {
    let repo = make_repo(VERIFICATION_CONFIG)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing generated main workflow")?;
    assert!(yaml.contains("cron: 17 3 * * *"), "schedule:\n{yaml}");
    assert!(yaml.contains("workflow_dispatch:"), "dispatch:\n{yaml}");
    assert!(yaml.contains("base_sha:"), "base SHA input:\n{yaml}");
    assert!(yaml.contains("scope:"), "scope input:\n{yaml}");
    assert!(
        yaml.contains("default: full"),
        "full scope default:\n{yaml}"
    );
    assert!(
        yaml.find("base_sha:") < yaml.find("scope:"),
        "dispatch inputs must be sorted:\n{yaml}"
    );
    Ok(())
}

#[test]
fn omitted_verification_keeps_main_triggers_unchanged() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing generated main workflow")?;
    assert!(!yaml.contains("cron: "), "unexpected schedule:\n{yaml}");
    assert!(
        !yaml.contains("workflow_dispatch:"),
        "unexpected dispatch:\n{yaml}"
    );
    Ok(())
}

#[test]
fn full_verification_skips_baseline_and_executes_universe() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": head,
        "head": head,
        "event": "workflow_dispatch",
        "scope": "full",
        "root": root.display().to_string(),
        "baseline_manifest": {"malformed": true},
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    assert_eq!(plan.scope, VerificationScope::Full);
    assert!(plan.base.is_none(), "full scope omits its baseline base");
    assert_eq!(plan.baseline.reason(), Some("full_verification"));
    assert!(!plan.warnings.iter().any(|warning| {
        warning.contains("malformed_manifest") || warning.contains("missing_base")
    }));
    assert!(!plan.obligations.is_empty(), "fixture must inventory work");
    assert!(
        plan.obligations
            .iter()
            .all(|obligation| obligation.decision == ObligationDecision::Execute)
    );
    let mut malformed = request;
    malformed["base"] = serde_json::json!("HEAD");
    let error = plan_internal(&malformed.to_string()).expect_err("bad full base");
    assert!(error.to_string().contains("bad_base"), "{error}");
    Ok(())
}

#[test]
fn full_and_scheduled_runs_ignore_a_baseline_that_covers_affected_dispatch() -> TestResult {
    use crate::impl_gates_cover::{entries_for, manifest_for, plan_with_manifest};

    let (repo, seed) = plan_with_manifest(None)?;
    let manifest = manifest_for(&seed, &seed.head, &entries_for(&seed));
    let mut request = serde_json::json!({
        "schema": 1, "run_key": "local", "base": seed.head, "head": seed.head,
        "event": "workflow_dispatch", "scope": "affected",
        "root": repo.path().display().to_string(), "baseline_manifest": manifest,
    });
    let response: serde_json::Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    let affected: Plan = serde_json::from_value(response["plan"].clone())?;
    assert!(!affected.obligations.is_empty());
    assert!(
        affected.obligations.iter().all(|obligation| {
            obligation.decision == ObligationDecision::CoveredByTrustedBaseline
        }),
        "{:?}",
        affected.warnings
    );
    assert!(affected.matrix.include.is_empty());
    request["scope"] = serde_json::json!("full");
    for event in ["workflow_dispatch", "schedule"] {
        request["event"] = serde_json::json!(event);
        let response: serde_json::Value =
            serde_json::from_str(&plan_internal(&request.to_string())?)?;
        let full: Plan = serde_json::from_value(response["plan"].clone())?;
        assert_eq!(full.task_ids, affected.task_ids);
        assert_eq!(full.baseline.reason(), Some("full_verification"));
        assert!(full.obligations.iter().all(|obligation| {
            obligation.decision == ObligationDecision::Execute
                && obligation.baseline_proof.is_none()
        }));
        assert!(!full.matrix.include.is_empty());
        assert!(response.get("baseline_manifest").is_none());
        assert!(
            !full
                .warnings
                .iter()
                .any(|warning| warning.starts_with("baseline_"))
        );
    }
    Ok(())
}

#[path = "impl_verification_observer.rs"]
mod verification_observer;
