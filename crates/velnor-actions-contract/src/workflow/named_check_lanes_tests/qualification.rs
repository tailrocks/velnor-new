use super::*;

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
