//! Schema-2 named-check lanes use distinct jobs, report identities and proofs.

use super::*;
use crate::StepRole;
use crate::workflow::{Concurrency, Job, JobTimeout, Permissions, Step, Trigger, WorkflowIr};
use std::collections::BTreeMap;
use velnor_actions_contract_config::config::{CheckExecutor, CheckPlatform, CheckRunner};

mod collision;
mod lanes;

fn runner(label: &str, platform: CheckPlatform) -> CheckRunner {
    CheckRunner {
        label: label.to_owned(),
        platform,
        executor: CheckExecutor::Hosted,
        container: None,
    }
}

fn job(id: &str, check_runner: Option<CheckRunner>, needs: &[&str]) -> Job {
    let steps = if id == "plan" {
        vec![Step {
            name: "Write request".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Internal {
                operation: "write-request-v1:plan-v1".to_owned(),
                env: BTreeMap::new(),
            },
        }]
    } else if check_runner.is_some() {
        vec![
            Step {
                name: "Execute named check".to_owned(),
                id: None,
                role: None,
                condition: None,
                kind: StepKind::Shell {
                    run: vec!["velnor-actions".to_owned()],
                    env: BTreeMap::from([
                        (NAMED_CHECK_JOB_ID_ENV.to_owned(), id.to_owned()),
                        (NAMED_CHECK_LANE_VARIANT_ENV.to_owned(), "single".to_owned()),
                    ]),
                },
            },
            Step {
                name: "Upload reports".to_owned(),
                id: None,
                role: Some(StepRole::MatrixReportUpload),
                condition: Some("always()".to_owned()),
                kind: StepKind::Action {
                    uses: "actions/upload-artifact@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_owned(),
                    with: BTreeMap::from([(
                        "name".to_owned(),
                        format!(
                            "velnor-crate-r${{{{ github.run_id }}}}-a${{{{ github.run_attempt }}}}-{id}"
                        ),
                    )]),
                    env: BTreeMap::new(),
                },
            },
        ]
    } else {
        vec![Step {
            name: "ordinary".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        }]
    };
    Job {
        outputs: Vec::new(),
        display_name: id.to_owned(),
        runs_on: check_runner
            .as_ref()
            .map_or_else(|| "ubuntu-26.04".to_owned(), |row| row.label.clone()),
        check_runner,
        timeout_minutes: JobTimeout::new(30).expect("timeout"),
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: None,
        permissions: Some(Permissions::default()),
        environment: None,
        steps,
    }
}

fn config(mode: ExecutionMode) -> velnor_actions_contract_config::config::VelnorConfig {
    let mode = match mode {
        ExecutionMode::Hosted => "hosted",
        ExecutionMode::ScaleSet => "scale-set",
        ExecutionMode::Both => "both",
    };
    serde_json::from_value(serde_json::json!({
        "schema":2,
        "workflow":{"name":"CI","policy":"consumer-v1","generator_validation":"bootstrap","max_parallel_jobs":4},
        "resources":{"compiler_process_budget":2,"test_process_budget":2},
        "test_sharding":{"default_shards":1,"by_manifest":{}},
        "stacks":{"ignore":[]},
        "discovery":{"exclude":[]},
        "execution":{
            "default_profile":"hosted","mode":mode,"hosted_profile":"hosted","scale_set_profile":"local",
            "profiles":{
                "hosted":{"kind":"github-hosted","label":"ubuntu-26.04","platform":"linux/amd64"},
                "local":{"kind":"github-scale-set","name":"orbstack-linux","labels":["velnor","orbstack-linux"],"platform":"linux/amd64"}
            },"workflows":[]
        }
    }))
    .expect("schema-2 fixture")
}

fn ir(checks: &[(&str, CheckRunner)]) -> WorkflowIr {
    let mut jobs = BTreeMap::new();
    jobs.insert("plan".to_owned(), job("plan", None, &[]));
    for (id, check_runner) in checks {
        jobs.insert(
            (*id).to_owned(),
            job(id, Some(check_runner.clone()), &["plan"]),
        );
    }
    jobs.insert(
        "required".to_owned(),
        job(
            "required",
            None,
            &checks.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        ),
    );
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs,
    }
}
