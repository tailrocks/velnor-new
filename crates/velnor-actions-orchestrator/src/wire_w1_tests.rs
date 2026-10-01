//! W1 emission wiring tests.
//!
//! Declared via `#[path]` from `wire_w1.rs` under `cfg(test)`.

use super::*;

#[test]
fn checkout_pins_canonical_ref_and_validates_inputs() {
    let step = checkout_step().expect("checkout step");
    let StepKind::Action { uses, with, .. } = &step.kind else {
        panic!("checkout must be an action step");
    };
    assert_eq!(uses, &PinnedActionRef::checkout().uses_value());
    assert_eq!(uses, crate::workflow::CHECKOUT_USES);
    assert!(validate_action_inputs(&checkout_inputs_schema(), with).is_ok());
    for (name, inputs) in [
        (
            "unknown",
            BTreeMap::from([("bogus".to_owned(), "x".to_owned())]),
        ),
        ("missing", BTreeMap::new()),
        (
            "empty",
            BTreeMap::from([("persist-credentials".to_owned(), String::new())]),
        ),
    ] {
        assert!(
            validate_action_inputs(&checkout_inputs_schema(), &inputs).is_err(),
            "{name} inputs must fail"
        );
    }
}

#[test]
fn checkout_full_provides_history_for_git_archaeology() {
    let step = checkout_step_full().expect("full checkout step");
    let StepKind::Action { uses, with, .. } = &step.kind else {
        panic!("checkout must be an action step");
    };
    assert_eq!(uses, &PinnedActionRef::checkout().uses_value());
    assert_eq!(
        with.get("fetch-depth").map(String::as_str),
        Some("0"),
        "plan checkout must clone full history: {with:?}"
    );
    assert!(validate_action_inputs(&checkout_inputs_schema(), with).is_ok());
    let shallow = checkout_step().expect("shallow checkout step");
    let StepKind::Action { with, .. } = &shallow.kind else {
        panic!("checkout must be an action step");
    };
    assert!(
        !with.contains_key("fetch-depth"),
        "non-plan checkouts stay shallow: {with:?}"
    );
}

#[test]
fn syntax_gate_allows_matrix_and_rejects_native() {
    assert!(vet_step_syntax(StepSyntax::JobMatrix).is_ok());
    let err = vet_step_syntax(StepSyntax::NativeParallelism).expect_err("native gated");
    assert!(err.to_string().contains("native_parallelism"), "{err}");
}

#[test]
fn task_cache_steps_need_fixture_and_live_mode() {
    assert!(maybe_task_cache_steps(None, TaskCacheMode::ReadWrite, "k").is_ok());
    assert!(
        maybe_task_cache_steps(None, TaskCacheMode::ReadWrite, "k")
            .expect("v")
            .is_empty()
    );
    let fixture = Gate6Fixture::new("gate6/w1").expect("fixture");
    assert!(
        maybe_task_cache_steps(Some(&fixture), TaskCacheMode::Off, "k")
            .expect("off")
            .is_empty()
    );
    let steps = maybe_task_cache_steps(Some(&fixture), TaskCacheMode::ReadOnly, "velnor-v1-task-k")
        .expect("gated steps");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].name, "Restore cache");
    assert_eq!(steps[1].name, "Save cache");
}

#[test]
fn crate_tools_follow_selection_with_validators() {
    use velnor_actions_mise::PinnedTool;

    use crate::matrix_step::{prepare_crate_tools_step, task_driver_tools};
    assert_eq!(task_driver_tools(false), vec![PinnedTool::Rust]);
    assert_eq!(
        task_driver_tools(true),
        vec![PinnedTool::Rust, PinnedTool::MrBoxington]
    );
    let catalog = ToolCatalog::pinned();
    for (use_mbx, use_nextest) in [(false, false), (false, true), (true, false), (true, true)] {
        let step = prepare_crate_tools_step(&catalog, use_mbx, use_nextest).expect("step");
        let StepKind::Shell { run, .. } = &step.kind else {
            panic!("prepare must be a shell step");
        };
        for tool in [
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
        ] {
            assert!(
                run.contains(&catalog.tool_spec(tool)),
                "crate jobs install {tool:?} for test-spawned generate: {run:?}"
            );
        }
        let nextest = catalog.tool_spec(PinnedTool::Nextest);
        assert_eq!(run.contains(&nextest), use_nextest);
        let mbx = catalog.tool_spec(PinnedTool::MrBoxington);
        assert_eq!(run.contains(&mbx), use_mbx);
        assert!(declared_config_variables().is_empty());
    }
}

#[test]
fn crate_tools_install_exact_pinned_set() {
    use velnor_actions_mise::PinnedTool;

    use crate::matrix_step::prepare_crate_tools_step;
    let catalog = ToolCatalog::pinned();
    for (use_mbx, use_nextest) in [(false, false), (false, true), (true, false), (true, true)] {
        let step = prepare_crate_tools_step(&catalog, use_mbx, use_nextest).expect("step");
        let StepKind::Shell { run, .. } = &step.kind else {
            panic!("prepare must be a shell step");
        };
        let at = run
            .iter()
            .position(|arg| arg == "install")
            .expect("install argv");
        let specs = &run[at + 1..];
        let mut expected = vec![catalog.tool_spec(PinnedTool::Rust)];
        if use_mbx {
            expected.push(catalog.tool_spec(PinnedTool::MrBoxington));
        }
        expected.extend([
            catalog.tool_spec(PinnedTool::Actionlint),
            catalog.tool_spec(PinnedTool::Shellcheck),
            catalog.tool_spec(PinnedTool::Zizmor),
        ]);
        if use_nextest {
            expected.push(catalog.tool_spec(PinnedTool::Nextest));
        }
        assert_eq!(
            specs,
            expected.as_slice(),
            "exact crate install set (mbx={use_mbx}, nextest={use_nextest})"
        );
        for absent in [PinnedTool::Gh, PinnedTool::ReleasePlz] {
            assert!(
                !specs.contains(&catalog.tool_spec(absent)),
                "crate jobs never install {absent:?}: {specs:?}"
            );
        }
    }
}
