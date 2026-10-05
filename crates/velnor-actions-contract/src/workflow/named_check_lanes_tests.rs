//! Schema-2 named-check lanes use distinct jobs, report identities and proofs.

use super::*;
use crate::config::{CheckExecutor, CheckPlatform, CheckRunner};
use crate::workflow::{Concurrency, Job, JobTimeout, Permissions, Step, Trigger, WorkflowIr};
use crate::{VelnorConfig, expand_workflow};
use std::collections::{BTreeMap, BTreeSet};

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
                role: None,
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

fn config(mode: ExecutionMode) -> VelnorConfig {
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

#[test]
fn both_emits_coherent_named_check_and_required_proof_lanes() {
    let source = ir(&[
        (
            "check-linux",
            runner("ubuntu-26.04", CheckPlatform::LinuxX64),
        ),
        ("check-mac", runner("macos-15", CheckPlatform::MacosArm64)),
    ]);
    let config = config(ExecutionMode::Both);
    let expanded = expand_workflow(&source, &config, None).expect("expanded workflow");
    let hosted = &expanded.jobs["check-linux__hosted"];
    let local = &expanded.jobs["check-linux__local"];
    assert_eq!(hosted.runs_on, "ubuntu-26.04");
    assert_eq!(local.runs_on, "scale-set:velnor+orbstack-linux");
    assert_eq!(hosted.condition, None);
    assert_eq!(
        local.condition.as_deref(),
        Some(crate::config::EPHEMERAL_CHECK_ADMISSION_CONDITION)
    );
    assert_eq!(expanded.jobs["check-mac"].runs_on, "macos-15");
    assert!(expanded.jobs.contains_key("check-mac"));
    let required: BTreeSet<&str> = expanded.jobs["required"]
        .needs
        .iter()
        .map(String::as_str)
        .collect();
    assert!(required.contains("check-linux__hosted"));
    assert!(required.contains("check-linux__local"));
    assert!(required.contains("check-mac"));

    let StepKind::Action {
        with: hosted_with, ..
    } = &hosted.steps[1].kind
    else {
        panic!("hosted upload step");
    };
    let StepKind::Action {
        with: local_with, ..
    } = &local.steps[1].kind
    else {
        panic!("local upload step");
    };
    assert_ne!(hosted_with["name"], local_with["name"]);
    assert!(hosted_with["name"].ends_with("-check-linux__hosted"));
    assert!(local_with["name"].ends_with("-check-linux__local"));
    let StepKind::Shell {
        env: hosted_env, ..
    } = &hosted.steps[0].kind
    else {
        panic!("hosted execution");
    };
    let StepKind::Shell { env: local_env, .. } = &local.steps[0].kind else {
        panic!("local execution");
    };
    assert_eq!(hosted_env[NAMED_CHECK_JOB_ID_ENV], "check-linux__hosted");
    assert_eq!(local_env[NAMED_CHECK_JOB_ID_ENV], "check-linux__local");
    assert_eq!(hosted_env[NAMED_CHECK_LANE_VARIANT_ENV], "hosted");
    assert_eq!(local_env[NAMED_CHECK_LANE_VARIANT_ENV], "scale_set");

    let expected = named_check_lanes(&source, &config, None).expect("lane map");
    assert_eq!(expected["check-linux"].len(), 2);
    assert_eq!(
        expected["check-linux"][0].variant,
        Some(NamedCheckLaneVariant::Hosted)
    );
    assert_eq!(
        expected["check-linux"][1].variant,
        Some(NamedCheckLaneVariant::ScaleSet)
    );
    assert_eq!(expected["check-mac"].len(), 1);
    let StepKind::Internal { env, .. } = &expanded.jobs["plan"].steps[0].kind else {
        panic!("plan request");
    };
    let encoded = &env[NAMED_CHECK_LANES_ENV];
    let decoded: BTreeMap<String, Vec<NamedCheckLane>> =
        serde_json::from_str(encoded).expect("typed lane map");
    assert_eq!(decoded, expected);
}

#[test]
fn expanded_job_id_collision_fails_instead_of_replacing_a_check() {
    let source = ir(&[
        ("check-foo", runner("ubuntu-26.04", CheckPlatform::LinuxX64)),
        (
            "check-foo__hosted",
            runner("macos-15", CheckPlatform::MacosArm64),
        ),
    ]);
    let error = expand_workflow(&source, &config(ExecutionMode::Both), None)
        .expect_err("both outputs collide with the fixed macOS check");
    assert!(
        error
            .to_string()
            .contains("expanded job id check-foo__hosted"),
        "{error}"
    );
}
