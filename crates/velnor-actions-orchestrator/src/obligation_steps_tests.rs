//! Closed action adaptation rejects drift and reports before cache export.

use super::*;
use velnor_actions_contract::StepKind;

fn model() -> CrateJob {
    CrateJob {
        job_id: "workload-app".to_owned(),
        display_name: "Workload / App".to_owned(),
        package_name: "app".to_owned(),
        package_id: "workload:app".to_owned(),
        manifest: ".".to_owned(),
        configuration: "docker_build".to_owned(),
        obligations: vec![CrateObligation {
            task_id: "stack/workload/app/build/docker_build".to_owned(),
            kind: "build".to_owned(),
            step_name: "Build".to_owned(),
            gated_by: Vec::new(),
            matrix_key: "m-0123456789abcdef".to_owned(),
            task_digest: format!("b3-{}", "a".repeat(64)),
            run: vec!["docker".to_owned(), "build".to_owned(), ".".to_owned()],
        }],
    }
}

#[test]
fn action_adapter_rejects_unreviewed_obligation_shapes() {
    let catalog = ToolCatalog::pinned();
    let mut model = model();
    let obligation = model.obligations[0].clone();
    let downstream = vec!["stack/workload/app/test/docker_build".to_owned()];
    assert!(steps(&model, &obligation, &catalog, &downstream, None).is_err());
    model.obligations.push(obligation.clone());
    assert!(steps(&model, &obligation, &catalog, &[], None).is_err());
    model.obligations.truncate(1);
    let mut unsupported = obligation.clone();
    unsupported.kind = "test".to_owned();
    assert!(steps(&model, &unsupported, &catalog, &[], None).is_err());
    unsupported = obligation.clone();
    unsupported.gated_by.push(obligation.task_id.clone());
    assert!(steps(&model, &unsupported, &catalog, &[], None).is_err());
    unsupported = obligation.clone();
    unsupported.matrix_key = "m-fedcba9876543210".to_owned();
    assert!(steps(&model, &unsupported, &catalog, &[], None).is_err());
    model.configuration = "bun_ci".to_owned();
    assert!(
        steps(&model, &obligation, &catalog, &[], None)
            .expect("ordinary")
            .is_none()
    );
}

#[test]
fn validation_reports_before_any_trusted_cache_export() {
    let model = model();
    let obligation = &model.obligations[0];
    let steps = steps(&model, obligation, &ToolCatalog::pinned(), &[], None)
        .expect("closed adapter")
        .expect("action");
    assert_eq!(steps.len(), 4);
    let id = format!("velnor-action-{}", obligation.matrix_key);
    assert_eq!(steps[1].id.as_ref().expect("outcome ID").as_str(), id);
    for index in [0, 2] {
        let condition = steps[index].condition.as_deref().expect("terminal gate");
        assert!(condition.contains("always()"));
        assert!(condition.contains(&format!(",{},", obligation.task_id)));
    }
    let StepKind::Action { with, env, .. } = &steps[1].kind else {
        panic!("native validation action");
    };
    assert!(!with.contains_key("cache-to"));
    assert_eq!(with["github-token"], "");
    assert_eq!(with["push"], "false");
    assert!(!with.contains_key("secrets"));
    assert!(!env.values().any(|value| value.contains("secrets.")));
    let StepKind::Shell { env, .. } = &steps[2].kind else {
        panic!("terminal report");
    };
    assert_eq!(
        env[ACTION_OUTCOME_ENV],
        format!("${{{{ steps.{id}.outcome }}}}")
    );
    let StepKind::Action { with, .. } = &steps[3].kind else {
        panic!("qualified cache export");
    };
    assert!(with["cache-to"].contains("ignore-error=true"));
    let gate = steps[3].condition.as_deref().expect("trusted export gate");
    assert!(gate.contains("success()"));
    assert!(gate.contains("github.event_name == 'push'"));
    assert!(gate.contains(&format!(",{},", obligation.task_id)));
}
