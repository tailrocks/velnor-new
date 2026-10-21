use velnor_actions_contract::{Step, StepKind, StepRole};

use super::*;
use crate::MiseSetup;

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

fn shell(name: &str, parallel: bool) -> Step {
    Step {
        parallel,
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn policy(steps: Vec<Step>) -> StackJobPolicy {
    StackJobPolicy {
        kind: StackJobKind::Dbt,
        id: "dbt-warehouse".to_owned(),
        display_name: "dbt / warehouse".to_owned(),
        timeout: JobTimeout::new(5).expect("timeout"),
        services: Vec::new(),
        setup: MiseSetup {
            uses: "jdx/mise-action@fc42bcc7ddbf51124c30f95a877a0c5168a3bfde".to_owned(),
            version: "2026.10.17".to_owned(),
            sha256: "0".repeat(64),
        },
        steps,
    }
}

#[test]
fn a_valid_policy_builds_a_gated_read_only_job() {
    let job =
        build_stack_job(&policy(vec![shell("a", false), shell("b", true)]), CHECKOUT).expect("job");
    assert_eq!(job.steps.len(), 4);
    assert_eq!(
        job.permissions.as_ref().map(|p| p.packages),
        Some(PermissionLevel::None)
    );
    assert!(
        job.condition
            .as_deref()
            .is_some_and(|c| c.contains("dbt-warehouse"))
    );
}

#[test]
fn privileged_split_or_foreign_steps_are_refused() {
    let mut token = shell("t", false);
    token.role = Some(StepRole::PrivateTokenMint);
    let mut split = vec![shell("a", true), shell("b", false), shell("c", true)];
    let cases = [
        (vec![], "stack_job_steps_empty"),
        (vec![token], "stack_job_privileged_step"),
        (std::mem::take(&mut split), "stack_parallel_group_split"),
    ];
    for (steps, code) in cases {
        let error = build_stack_job(&policy(steps), CHECKOUT).expect_err(code);
        assert!(error.to_string().contains(code), "{code}: {error}");
    }
    let mut foreign = policy(vec![shell("a", false)]);
    foreign.id = "ansible-x".to_owned();
    assert!(build_stack_job(&foreign, CHECKOUT).is_err());
}
