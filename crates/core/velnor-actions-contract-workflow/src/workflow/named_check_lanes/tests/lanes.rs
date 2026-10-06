use super::super::*;
use super::{config, ir, runner};
use crate::expand_workflow;
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract_config::config::CheckPlatform;

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
        Some(velnor_actions_contract_config::config::EPHEMERAL_CHECK_ADMISSION_CONDITION)
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
