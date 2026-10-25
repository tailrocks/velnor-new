use super::stable::{
    stable_head_formula_is_activated, stable_install_required, stable_or_preview_install_check,
};
use super::{
    actual_native_checks, require_native_authority, route_required_admission,
    validate_required_matrix_entry,
};
use std::cell::Cell;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, Plan, PlanGenerator, PlanMatrix, PlanRunner,
    PlannedPlatform, RunnerSelection, Trust, WorkflowEvent,
    config::{
        CheckExecutor, CheckPlatform, CheckRunner, NativeHomebrewCheck, NativeHomebrewOperation,
    },
    plan_id_for_run,
};

fn plan(event: WorkflowEvent, entries: Vec<MatrixEntry>) -> Plan {
    Plan {
        schema: Plan::SCHEMA,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan ID"),
        base: None,
        head: "a".repeat(40),
        event,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        generator: PlanGenerator {
            version: "1.0.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "b".repeat(64),
        },
        packages: Vec::new(),
        obligations: Vec::new(),
        matrix: PlanMatrix { include: entries },
        task_ids: Vec::new(),
        jobs: Vec::new(),
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

fn malformed_native_entry() -> MatrixEntry {
    let digest = format!("b3-{}", "c".repeat(64));
    MatrixEntry::derive(
        "native",
        "internal/native-check/tap_tree",
        "native-homebrew-check-v1",
        &digest,
        serde_json::json!({"incomplete": true}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "native_check".to_owned(),
                ExecuteTaskRef::Single("internal/native-check/tap_tree".to_owned()),
            )]),
        },
        &digest,
        "local",
        "check-tap-tree",
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform"),
    )
    .expect("raw native matrix entry")
}

#[test]
fn fork_and_merge_group_absence_skip_the_full_admission() {
    for event in [WorkflowEvent::Fork, WorkflowEvent::MergeGroup] {
        let plan = plan(event, Vec::new());
        let absence_calls = Cell::new(0);
        let full_calls = Cell::new(0);
        let result = route_required_admission(
            &plan,
            || {
                absence_calls.set(absence_calls.get() + 1);
                Ok(true)
            },
            || {
                full_calls.set(full_calls.get() + 1);
                Err(crate::internal::internal("unexpected_full_admission"))
            },
        );
        assert!(result.is_ok());
        assert_eq!(absence_calls.get(), 1);
        assert_eq!(full_calls.get(), 0);
    }
}

#[test]
fn present_locator_runs_full_admission_and_propagates_its_failure() {
    let plan = plan(WorkflowEvent::PullRequest, Vec::new());
    let full_calls = Cell::new(0);
    let result = route_required_admission(
        &plan,
        || Ok(false),
        || {
            full_calls.set(full_calls.get() + 1);
            Err(crate::internal::internal("full_admission_rejected"))
        },
    );
    assert!(result.is_err());
    assert_eq!(full_calls.get(), 1);
}

#[test]
fn raw_native_entry_with_malformed_metadata_never_uses_the_absence_shortcut() {
    let entry = malformed_native_entry();
    let entries = vec![entry];
    assert!(actual_native_checks(&entries).is_empty());
    assert!(require_native_authority(&entries).is_err());
    let plan = plan(WorkflowEvent::PullRequest, entries);
    let absence_calls = Cell::new(0);
    let full_calls = Cell::new(0);
    let result = route_required_admission(
        &plan,
        || {
            absence_calls.set(absence_calls.get() + 1);
            Ok(true)
        },
        || {
            full_calls.set(full_calls.get() + 1);
            Err(crate::internal::internal("full_admission_rejected"))
        },
    );
    assert!(result.is_err());
    assert_eq!(absence_calls.get(), 0);
    assert_eq!(full_calls.get(), 1);
}

#[test]
fn locator_read_error_propagates_without_falling_back_or_accepting() {
    let plan = plan(WorkflowEvent::Push, Vec::new());
    let full_calls = Cell::new(0);
    let result = route_required_admission(
        &plan,
        || Err(crate::internal::internal("locator_read_failed")),
        || {
            full_calls.set(full_calls.get() + 1);
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(full_calls.get(), 0);
}

#[test]
fn missing_protected_native_matrix_entry_fails_the_required_validator() {
    let required = NativeHomebrewCheck {
        id: "tap_tree".to_owned(),
        operation: NativeHomebrewOperation::TapTree,
        formula_channel: None,
        runner: CheckRunner {
            label: "ubuntu-26.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        timeout_minutes: 5,
    };
    assert!(validate_required_matrix_entry(&BTreeMap::new(), &required).is_err());
}

fn stable_formula_fixture() -> (
    velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    velnor_actions_contract::PackageReleasePlan,
) {
    let spec = serde_json::from_slice(include_bytes!(
        "../../../.velnor/qualification/homebrew/formula-spec.json"
    ))
    .expect("typed protected formula spec");
    let mut plan = velnor_actions_contract::parse_package_release_plan(include_bytes!(
        "../../../.velnor/qualification/homebrew/preview-plan.json"
    ))
    .expect("typed protected package plan");
    plan.channel = velnor_actions_contract::package_release::PackageReleaseChannel::Stable;
    (spec, plan)
}

fn active_stable_formula(
    spec: &velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    plan: &velnor_actions_contract::PackageReleasePlan,
) -> Vec<u8> {
    use velnor_actions_contract::homebrew_spec::{
        HomebrewFormulaReleaseLiterals, render_homebrew_formula, validate_homebrew_projection,
    };
    let projection = validate_homebrew_projection(spec, plan).expect("stable projection");
    let digests = projection
        .selected_payloads()
        .iter()
        .map(|payload| (payload.asset_name().to_owned(), "c".repeat(64)))
        .collect();
    let literals = HomebrewFormulaReleaseLiterals::new(
        spec,
        &projection,
        "b12e73afd75b944a872e8295d61a15f93f5789a2".to_owned(),
        "0.6.4".to_owned(),
        "v0.6.4".to_owned(),
        digests,
    )
    .expect("active stable formula literals");
    render_homebrew_formula(spec, &literals).expect("canonical active stable formula")
}

#[test]
fn stable_head_activation_requires_the_arm_install_check() {
    use velnor_actions_contract::homebrew_spec::{
        HomebrewFormulaReleaseLiterals, render_homebrew_formula, validate_homebrew_projection,
    };
    use velnor_actions_contract::package_release::PackageReleaseChannel;

    let (spec, plan) = stable_formula_fixture();
    let projection = validate_homebrew_projection(&spec, &plan).expect("stable projection");
    let sentinel = HomebrewFormulaReleaseLiterals::stable_sentinel(&spec, &projection)
        .expect("disabled stable sentinel");
    let sentinel_bytes = render_homebrew_formula(&spec, &sentinel).expect("sentinel formula");
    let active_bytes = active_stable_formula(&spec, &plan);

    let base_active = stable_head_formula_is_activated(&spec, &plan, Some(&sentinel_bytes))
        .expect("sentinel base is inactive");
    let same_head_active = stable_head_formula_is_activated(&spec, &plan, Some(&sentinel_bytes))
        .expect("sentinel head is inactive");
    assert!(!stable_install_required(base_active, same_head_active));

    let head_active = stable_head_formula_is_activated(&spec, &plan, Some(&active_bytes))
        .expect("active authenticated head formula");
    assert!(stable_install_required(base_active, head_active));
    assert!(stable_install_required(head_active, same_head_active));
    let required = stable_or_preview_install_check(PackageReleaseChannel::Stable);
    assert_eq!(required.id, "install_stable");
    assert_eq!(required.timeout_minutes, 30);
    assert_eq!(required.runner.label, "macos-15");
    assert_eq!(required.runner.platform, CheckPlatform::MacosArm64);
    assert!(validate_required_matrix_entry(&BTreeMap::new(), &required).is_err());
    let actual = BTreeMap::from([(
        "install_stable".to_owned(),
        (
            "install".to_owned(),
            Some("stable".to_owned()),
            "macos_arm64".to_owned(),
            "macos-15".to_owned(),
        ),
    )]);
    validate_required_matrix_entry(&actual, &required).expect("matching stable install matrix");
}

#[test]
fn malformed_or_missing_authenticated_stable_head_formula_fails_closed() {
    let (spec, plan) = stable_formula_fixture();
    assert!(stable_head_formula_is_activated(&spec, &plan, None).is_err());
    assert!(stable_head_formula_is_activated(&spec, &plan, Some(b"not a formula\n")).is_err());
}
