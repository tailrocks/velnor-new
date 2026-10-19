use super::*;

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
fn named_check_uses_one_job_across_qualification_events() {
    let source = ir(&[(
        "check-linux",
        runner("ubuntu-26.04", CheckPlatform::LinuxX64),
    )]);
    let scale_set = "scale-set:velnor+orbstack-linux";
    let cases = [
        (ExecutionMode::Hosted, vec![("check-linux", "ubuntu-26.04")]),
        (ExecutionMode::ScaleSet, vec![("check-linux", scale_set)]),
        (
            ExecutionMode::Both,
            vec![
                ("check-linux__hosted", "ubuntu-26.04"),
                ("check-linux__local", scale_set),
            ],
        ),
    ];

    for (mode, expected_lanes) in cases {
        let config = config(mode);
        let expanded = expand_workflow(&source, &config, None).expect("expanded workflow");
        let lanes = named_check_lanes(&source, &config, None).expect("qualification lane map");
        let qualification_lanes = &lanes["check-linux"];
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
            let expected_condition = (*runs_on == scale_set)
                .then_some(crate::config::EPHEMERAL_CHECK_ADMISSION_CONDITION);
            assert_eq!(job.condition.as_deref(), expected_condition, "{job_id}");
        }

        let StepKind::Internal { env, .. } = &expanded.jobs["plan"].steps[0].kind else {
            panic!("plan request");
        };
        let encoded = &env[NAMED_CHECK_LANES_ENV];
        let decoded: BTreeMap<String, Vec<NamedCheckLane>> =
            serde_json::from_str(encoded).expect("typed lane map");
        assert_eq!(
            decoded["check-linux"], *qualification_lanes,
            "plan receives the same lane map used by Required for {mode:?}"
        );
    }
}

#[test]
fn named_check_uses_its_package_job_override() {
    let source = ir(&[(
        "check-linux",
        runner("ubuntu-26.04", CheckPlatform::LinuxX64),
    )]);
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
        expanded.jobs["check-linux"].runs_on,
        "scale-set:velnor+orbstack-linux"
    );
    assert_eq!(
        expanded.jobs["check-linux"].condition.as_deref(),
        Some(crate::config::EPHEMERAL_CHECK_ADMISSION_CONDITION)
    );
    let lanes = named_check_lanes(&source, &config, None).expect("qualification lane map");
    assert_eq!(
        lanes["check-linux"][0].runs_on.as_deref(),
        Some("scale-set:velnor+orbstack-linux")
    );
}
