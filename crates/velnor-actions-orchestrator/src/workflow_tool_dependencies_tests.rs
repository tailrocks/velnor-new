use super::depend_on_producer;
use velnor_actions_contract::{Job, JobTimeout};

fn consumer(needs: Vec<String>, condition: Option<String>) -> Job {
    Job {
        display_name: "consumer".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs,
        condition,
        cache_mode: Some(velnor_actions_contract::CacheMode::Read),
        outputs: Vec::new(),
        permissions: None,
        environment: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        tool_producer: None,
        mbx_producer: None,
        steps: Vec::new(),
    }
}

#[test]
fn planning_dependency_does_not_introduce_plan_cycle() {
    let mut job = consumer(Vec::new(), None);
    depend_on_producer(&mut job, "planning-tools");
    assert_eq!(job.needs, ["planning-tools"]);
    let condition = job.condition.expect("condition");
    assert!(!condition.contains("needs.plan"));
    assert!(condition.contains("!cancelled()"));
    assert!(!condition.contains("planning-tools"));
    assert!(!condition.contains("success()"));
}

#[test]
fn selected_consumer_retains_selection_and_original_dependency_gate() {
    let mut job = consumer(
        vec!["plan".to_owned()],
        Some(
            "success() && !contains(needs.plan.outputs.covered_tasks, ',rust:test:a,')".to_owned(),
        ),
    );
    depend_on_producer(&mut job, "full-tools");
    let condition = job.condition.clone().expect("condition");
    assert!(condition.contains("needs['plan'].result == 'success'"));
    assert!(condition.contains("!contains(needs.plan.outputs.covered_tasks"));
    assert!(!condition.contains("success()"));
    depend_on_producer(&mut job, "full-tools");
    assert_eq!(job.needs, ["plan", "full-tools"]);
    assert_eq!(job.condition.as_deref(), Some(condition.as_str()));
}
