//! Schema-2 named-check lanes use distinct jobs, report identities and proofs.

use super::*;
use crate::config::{CheckExecutor, CheckPlatform, CheckRunner, ExecutionOverride, ExecutionRole};
use crate::workflow::{Concurrency, Job, JobTimeout, Permissions, Step, Trigger, WorkflowIr};
use crate::{VelnorConfig, expand_workflow, expand_workflow_with_qualification_sources};
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
        condition: id
            .starts_with(QUALIFICATION_CHECK_JOB_PREFIX)
            .then(|| crate::config::QUALIFICATION_CHECK_ADMISSION_CONDITION.to_owned()),
        permissions: Some(Permissions::default()),
        cache_mode: None,
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
    jobs.insert(
        "publish-baseline".to_owned(),
        job("publish-baseline", None, &["required"]),
    );
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs,
    }
}

fn qualification_crate_ir() -> WorkflowIr {
    let mut source = ir(&[]);
    let qualification_ids = [
        "rust-qualification-0123456789abcdef",
        "tofu-qualification-fedcba9876543210",
    ];
    for id in ["rust-demo", "tofu-demo"]
        .into_iter()
        .chain(qualification_ids)
    {
        source.jobs.insert(id.to_owned(), job(id, None, &["plan"]));
    }
    source.jobs.get_mut("required").expect("Required job").needs =
        qualification_ids.iter().map(ToString::to_string).collect();
    source
}

fn mixed_qualification_ir() -> WorkflowIr {
    let check_runner = runner("ubuntu-26.04", CheckPlatform::LinuxX64);
    let mut source = qualification_crate_ir();
    for id in [
        "check-java",
        "qualification-check-java",
        "check-frontend",
        "qualification-check-frontend",
    ] {
        source.jobs.insert(
            id.to_owned(),
            job(id, Some(check_runner.clone()), &["plan"]),
        );
    }
    source.jobs.get_mut("required").expect("Required job").needs = [
        "rust-qualification-0123456789abcdef",
        "tofu-qualification-fedcba9876543210",
        "qualification-check-java",
        "qualification-check-frontend",
    ]
    .map(ToString::to_string)
    .to_vec();
    source
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
    for id in ["plan", "required", "publish-baseline"] {
        assert_eq!(expanded.jobs[id].runs_on, "ubuntu-26.04", "{id}");
    }
    assert_eq!(expanded.jobs["publish-baseline"].needs, ["required"]);
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
fn qualification_named_checks_follow_hosted_scale_set_and_both_placement() {
    let source = ir(&[
        (
            "check-linux",
            runner("ubuntu-26.04", CheckPlatform::LinuxX64),
        ),
        (
            "qualification-check-linux",
            runner("ubuntu-26.04", CheckPlatform::LinuxX64),
        ),
    ]);
    let scale_set = "scale-set:velnor+orbstack-linux";
    let cases = [
        (
            ExecutionMode::Hosted,
            vec![("qualification-check-linux", "ubuntu-26.04")],
        ),
        (
            ExecutionMode::ScaleSet,
            vec![("qualification-check-linux", scale_set)],
        ),
        (
            ExecutionMode::Both,
            vec![
                ("qualification-check-linux__hosted", "ubuntu-26.04"),
                ("qualification-check-linux__local", scale_set),
            ],
        ),
    ];

    for (mode, expected_lanes) in cases {
        let config = config(mode);
        let expanded = expand_workflow(&source, &config, None).expect("expanded workflow");
        let lanes = named_check_lanes(&source, &config, None).expect("qualification lane map");
        let qualification_lanes = &lanes["qualification-check-linux"];
        assert_eq!(
            qualification_lanes
                .iter()
                .map(|lane| (
                    lane.job_id.as_str(),
                    lane.runs_on.as_deref().unwrap_or_default()
                ))
                .collect::<Vec<_>>(),
            expected_lanes,
            "plan lane map for {mode:?}"
        );

        let required = &expanded.jobs["required"];
        for (job_id, runs_on) in &expected_lanes {
            assert!(required.needs.iter().any(|need| need == job_id), "{job_id}");
            let job = &expanded.jobs[*job_id];
            assert_eq!(job.runs_on, *runs_on, "{job_id}");
            assert_eq!(
                job.condition.as_deref(),
                Some(crate::config::QUALIFICATION_CHECK_ADMISSION_CONDITION),
                "{job_id}"
            );
        }

        let StepKind::Internal { env, .. } = &expanded.jobs["plan"].steps[0].kind else {
            panic!("plan request");
        };
        let encoded = &env[NAMED_CHECK_LANES_ENV];
        let decoded: BTreeMap<String, Vec<NamedCheckLane>> =
            serde_json::from_str(encoded).expect("typed lane map");
        assert_eq!(
            decoded["qualification-check-linux"], *qualification_lanes,
            "plan receives the same lane map used by Required for {mode:?}"
        );
    }
}

#[test]
fn qualification_named_check_inherits_its_source_override() {
    let source = ir(&[
        (
            "check-linux",
            runner("ubuntu-26.04", CheckPlatform::LinuxX64),
        ),
        (
            "qualification-check-linux",
            runner("ubuntu-26.04", CheckPlatform::LinuxX64),
        ),
    ]);
    let mut config = config(ExecutionMode::Hosted);
    let execution = config.execution.as_mut().expect("execution config");
    execution.mode = None;
    execution.overrides.insert(
        "check-linux".to_owned(),
        ExecutionOverride {
            profile: "local".to_owned(),
            role: ExecutionRole::Verification,
        },
    );

    let expanded = expand_workflow(&source, &config, None).expect("source override applies");
    assert_eq!(
        expanded.jobs["qualification-check-linux"].runs_on,
        "scale-set:velnor+orbstack-linux"
    );
    assert_eq!(
        expanded.jobs["qualification-check-linux"]
            .condition
            .as_deref(),
        Some(crate::config::QUALIFICATION_CHECK_ADMISSION_CONDITION)
    );
    let lanes = named_check_lanes(&source, &config, None).expect("qualification lane map");
    assert_eq!(
        lanes["qualification-check-linux"][0].runs_on.as_deref(),
        Some("scale-set:velnor+orbstack-linux")
    );
}

#[test]
fn qualification_crate_lanes_match_hosted_scale_set_and_both_jobs() {
    let source = qualification_crate_ir();
    let sources = BTreeMap::from([
        (
            "rust-qualification-0123456789abcdef".to_owned(),
            "rust-demo".to_owned(),
        ),
        (
            "tofu-qualification-fedcba9876543210".to_owned(),
            "tofu-demo".to_owned(),
        ),
    ]);
    let scale_set = "scale-set:velnor+orbstack-linux";
    for (mode, expected) in [
        (
            ExecutionMode::Hosted,
            vec![
                ("rust-qualification-0123456789abcdef", "ubuntu-26.04"),
                ("tofu-qualification-fedcba9876543210", "ubuntu-26.04"),
            ],
        ),
        (
            ExecutionMode::ScaleSet,
            vec![
                ("rust-qualification-0123456789abcdef", scale_set),
                ("tofu-qualification-fedcba9876543210", scale_set),
            ],
        ),
        (
            ExecutionMode::Both,
            vec![
                (
                    "rust-qualification-0123456789abcdef__hosted",
                    "ubuntu-26.04",
                ),
                ("rust-qualification-0123456789abcdef__local", scale_set),
                (
                    "tofu-qualification-fedcba9876543210__hosted",
                    "ubuntu-26.04",
                ),
                ("tofu-qualification-fedcba9876543210__local", scale_set),
            ],
        ),
    ] {
        let config = config(mode);
        let lanes = qualification_job_lanes(&source, &config, None, &sources)
            .expect("Qualification lane map");
        let expanded = expand_workflow_with_qualification_sources(&source, &config, None, &sources)
            .expect("expanded workflow");
        let required: BTreeSet<&str> = expanded.jobs["required"]
            .needs
            .iter()
            .map(String::as_str)
            .collect();
        let actual: Vec<(&str, &str)> = lanes
            .values()
            .flatten()
            .map(|lane| {
                (
                    lane.job_id.as_str(),
                    lane.runs_on.as_deref().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(actual, expected);
        for (job_id, runs_on) in &expected {
            assert!(required.contains(job_id), "Required omits {job_id}");
            assert_eq!(expanded.jobs[*job_id].runs_on, *runs_on, "{job_id}");
        }
        let StepKind::Internal { env, .. } = &expanded.jobs["plan"].steps[0].kind else {
            panic!("plan request");
        };
        let encoded = &env[NAMED_CHECK_LANES_ENV];
        let decoded: BTreeMap<String, Vec<NamedCheckLane>> =
            serde_json::from_str(encoded).expect("typed lane map");
        assert_eq!(decoded, lanes);
    }
}

#[test]
fn qualification_crate_lanes_inherit_source_runner_override() {
    let source = qualification_crate_ir();
    let sources = BTreeMap::from([
        (
            "rust-qualification-0123456789abcdef".to_owned(),
            "rust-demo".to_owned(),
        ),
        (
            "tofu-qualification-fedcba9876543210".to_owned(),
            "tofu-demo".to_owned(),
        ),
    ]);
    let mut config = config(ExecutionMode::Hosted);
    let execution = config.execution.as_mut().expect("execution");
    execution.mode = None;
    execution.overrides.insert(
        "rust-demo".to_owned(),
        ExecutionOverride {
            profile: "local".to_owned(),
            role: ExecutionRole::Verification,
        },
    );
    let lanes =
        qualification_job_lanes(&source, &config, None, &sources).expect("Qualification lane map");
    assert_eq!(
        lanes["rust-qualification-0123456789abcdef"][0]
            .runs_on
            .as_deref(),
        Some("scale-set:velnor+orbstack-linux")
    );
}

#[test]
fn mixed_qualification_jobs_keep_required_and_plan_lanes_aligned() {
    let source = mixed_qualification_ir();
    let sources = BTreeMap::from([
        (
            "rust-qualification-0123456789abcdef".to_owned(),
            "rust-demo".to_owned(),
        ),
        (
            "tofu-qualification-fedcba9876543210".to_owned(),
            "tofu-demo".to_owned(),
        ),
    ]);
    for mode in [
        ExecutionMode::Hosted,
        ExecutionMode::ScaleSet,
        ExecutionMode::Both,
    ] {
        let config = config(mode);
        let mut lanes = named_check_lanes(&source, &config, None).expect("named-check lanes");
        for (id, entries) in
            qualification_job_lanes(&source, &config, None, &sources).expect("crate lanes")
        {
            assert!(lanes.insert(id, entries).is_none());
        }
        let expanded = expand_workflow_with_qualification_sources(&source, &config, None, &sources)
            .expect("expanded workflow");
        let required: BTreeSet<&str> = expanded.jobs["required"]
            .needs
            .iter()
            .map(String::as_str)
            .collect();
        for id in [
            "rust-qualification-0123456789abcdef",
            "tofu-qualification-fedcba9876543210",
            "qualification-check-java",
            "qualification-check-frontend",
        ] {
            for lane in &lanes[id] {
                assert!(
                    required.contains(lane.job_id.as_str()),
                    "{mode:?}: {lane:?}"
                );
                assert!(
                    expanded.jobs.contains_key(&lane.job_id),
                    "{mode:?}: {lane:?}"
                );
            }
        }
        let StepKind::Internal { env, .. } = &expanded.jobs["plan"].steps[0].kind else {
            panic!("plan request");
        };
        let decoded: BTreeMap<String, Vec<NamedCheckLane>> =
            serde_json::from_str(&env[NAMED_CHECK_LANES_ENV]).expect("typed lane map");
        assert_eq!(decoded, lanes, "plan lane map for {mode:?}");
    }
}

#[test]
fn hosted_mode_keeps_configured_runners_and_one_writer() {
    let source = ir(&[
        (
            "check-linux",
            runner("ubuntu-26.04", CheckPlatform::LinuxX64),
        ),
        ("check-mac", runner("macos-15", CheckPlatform::MacosArm64)),
    ]);
    let expanded = expand_workflow(&source, &config(ExecutionMode::Hosted), None)
        .expect("hosted mode keeps configured runners");

    assert_eq!(expanded.jobs.len(), source.jobs.len());
    assert_eq!(expanded.jobs["plan"].runs_on, "ubuntu-26.04");
    assert_eq!(expanded.jobs["check-linux"].runs_on, "ubuntu-26.04");
    assert_eq!(expanded.jobs["check-mac"].runs_on, "macos-15");
    assert_eq!(expanded.jobs["required"].runs_on, "ubuntu-26.04");
    assert_eq!(expanded.jobs["publish-baseline"].runs_on, "ubuntu-26.04");
}

#[test]
fn scale_set_routes_linux_jobs_once_including_controls_and_writers() {
    let source = ir(&[(
        "check-linux",
        runner("ubuntu-26.04", CheckPlatform::LinuxX64),
    )]);
    let expanded = expand_workflow(&source, &config(ExecutionMode::ScaleSet), None)
        .expect("scale-set mode routes every Linux job");
    let runner = "scale-set:velnor+orbstack-linux";

    assert_eq!(expanded.jobs.len(), 4);
    for id in ["plan", "check-linux", "required", "publish-baseline"] {
        assert_eq!(expanded.jobs[id].runs_on, runner, "{id}");
    }
    assert_eq!(expanded.jobs["plan"].needs, Vec::<String>::new());
    assert_eq!(expanded.jobs["required"].needs, ["check-linux"]);
    assert_eq!(expanded.jobs["publish-baseline"].needs, ["required"]);
    assert_eq!(
        expanded.jobs["check-linux"].condition.as_deref(),
        Some(crate::config::EPHEMERAL_CHECK_ADMISSION_CONDITION)
    );
}

#[test]
fn scale_set_mode_rejects_darwin_jobs_with_platform_evidence() {
    let source = ir(&[("check-mac", runner("macos-15", CheckPlatform::MacosArm64))]);
    let error = expand_workflow(&source, &config(ExecutionMode::ScaleSet), None)
        .expect_err("Linux scale set cannot run a Darwin job");
    assert!(
        error
            .to_string()
            .contains("unsupported_darwin_workload:check-mac:macos-15"),
        "{error}"
    );
}

#[test]
fn scale_set_default_profile_routes_control_and_writer_roles() {
    let source = ir(&[(
        "check-linux",
        runner("ubuntu-26.04", CheckPlatform::LinuxX64),
    )]);
    let mut config = config(ExecutionMode::Hosted);
    let execution = config.execution.as_mut().expect("execution config");
    execution.mode = None;
    execution.default_profile = "local".to_owned();

    let expanded = expand_workflow(&source, &config, None).expect("default Scale Set profile");
    for id in ["plan", "check-linux", "required", "publish-baseline"] {
        assert_eq!(
            expanded.jobs[id].runs_on, "scale-set:velnor+orbstack-linux",
            "{id}"
        );
    }
}

#[test]
fn role_overrides_route_control_and_writer_jobs_when_mode_is_unset() {
    let source = ir(&[(
        "check-linux",
        runner("ubuntu-26.04", CheckPlatform::LinuxX64),
    )]);
    let mut config = config(ExecutionMode::Hosted);
    let execution = config.execution.as_mut().expect("execution config");
    execution.mode = None;
    execution.overrides.insert(
        "plan".to_owned(),
        ExecutionOverride {
            profile: "local".to_owned(),
            role: ExecutionRole::Control,
        },
    );
    execution.overrides.insert(
        "publish-baseline".to_owned(),
        ExecutionOverride {
            profile: "local".to_owned(),
            role: ExecutionRole::Release,
        },
    );

    let expanded = expand_workflow(&source, &config, None).expect("role-scoped overrides");
    assert_eq!(
        expanded.jobs["plan"].runs_on,
        "scale-set:velnor+orbstack-linux"
    );
    assert_eq!(expanded.jobs["check-linux"].runs_on, "ubuntu-26.04");
    assert_eq!(expanded.jobs["required"].runs_on, "ubuntu-26.04");
    assert_eq!(
        expanded.jobs["publish-baseline"].runs_on,
        "scale-set:velnor+orbstack-linux"
    );
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
