//! Descriptive helper fixtures never mint tool, action or origin admission.
use super::*;
use velnor_actions_contract::{Job, JobTimeout, StepKind};

fn plan(steps: Vec<Step>) -> Job {
    Job {
        cache_mode: None,
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps,
    }
}

fn actual_steps(catalog: &ToolCatalog) -> Vec<Step> {
    let homes = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true).expect("homes");
    vec![
        preseed_build_step(
            &crate::vectors::candidate_build_argv(catalog).expect("build"),
            &homes,
        )
        .expect("build step"),
        preseed_verify_step(
            &crate::vectors::mbx_probe_argv(catalog).expect("probe"),
            catalog.version(PinnedTool::MrBoxington),
            &homes,
        )
        .expect("probe step"),
    ]
}

#[test]
fn current_source_recipe_requires_exact_real_build_then_probe() {
    let catalog = ToolCatalog::pinned();
    attached_recipe(&plan(actual_steps(&catalog)), &catalog).expect("real recipe");
    let mut changed = actual_steps(&catalog);
    let StepKind::Shell { run, .. } = &mut changed[0].kind else {
        panic!("build shell");
    };
    run.push("--foreign-flag".to_owned());
    assert!(attached_recipe(&plan(changed), &catalog).is_err());
    let mut reversed = actual_steps(&catalog);
    reversed.reverse();
    assert!(attached_recipe(&plan(reversed), &catalog).is_err());
    let mut duplicate = actual_steps(&catalog);
    duplicate.push(duplicate[0].clone());
    assert!(attached_recipe(&plan(duplicate), &catalog).is_err());
    assert!(attached_recipe(&plan(Vec::new()), &catalog).is_err());
}

#[test]
fn pending_helper_inputs_retain_their_distinct_domain_and_job() {
    let result = pending("mbx_helper_current_source_build_pending");
    assert!(result.domains.is_empty());
    assert!(result.source_helpers.is_empty());
    assert!(result.receipt_drafts.is_empty());
    assert_eq!(result.unsupported.len(), 1);
    assert_eq!(result.unsupported[0].domain, MbxCacheDomain::Helper);
    assert_eq!(result.unsupported[0].job_id, "plan");
    assert_eq!(
        result.unsupported[0].reason,
        "mbx_helper_current_source_build_pending"
    );
    assert!(QualifiedMbxAction::require_comparison_export().is_err());
}
