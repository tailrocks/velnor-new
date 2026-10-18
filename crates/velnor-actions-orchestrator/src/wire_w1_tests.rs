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
    assert_eq!(
        maybe_task_cache_steps(None, TaskCacheMode::ReadWrite, "k").expect("v"),
        [] as [velnor_actions_contract::Step; 0]
    );
    let fixture = Gate6Fixture::new("gate6/w1").expect("fixture");
    assert_eq!(
        maybe_task_cache_steps(Some(&fixture), TaskCacheMode::Off, "k").expect("off"),
        [] as [velnor_actions_contract::Step; 0]
    );
    let steps = maybe_task_cache_steps(Some(&fixture), TaskCacheMode::ReadOnly, "velnor-v1-task-k")
        .expect("gated steps");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].name, "Restore cache");
    assert_eq!(steps[1].name, "Save cache");
}

#[test]
fn crate_driver_tools_follow_role_selection() {
    use velnor_actions_mise::PinnedTool;

    use crate::matrix_step::task_driver_tools;
    assert_eq!(task_driver_tools(true, false), vec![PinnedTool::Rust]);
    assert_eq!(task_driver_tools(true, false), vec![PinnedTool::Rust]);
    assert_eq!(
        task_driver_tools(true, true),
        vec![PinnedTool::Rust, PinnedTool::Opentofu]
    );
    assert_eq!(
        task_driver_tools(true, true),
        vec![PinnedTool::Rust, PinnedTool::Opentofu]
    );
    assert_eq!(
        task_driver_tools(false, true),
        vec![PinnedTool::Opentofu],
        "pure-tofu drivers carry no Rust"
    );
    assert!(
        task_driver_tools(false, false).is_empty(),
        "no drivers without any role"
    );
}

#[test]
fn crate_tools_follow_selection_with_validators() {
    use velnor_actions_mise::PinnedTool;

    use crate::matrix_step::prepare_crate_tools_step;
    let catalog = ToolCatalog::pinned();
    for (use_rust, use_mbx, use_nextest, use_opentofu, needs_validators) in [
        (true, false, false, false, false),
        (true, false, true, true, true),
        (true, true, false, true, true),
        (true, true, true, false, false),
        (false, false, false, true, false),
        (false, false, false, true, true),
    ] {
        let step = prepare_crate_tools_step(
            &catalog,
            use_rust,
            use_nextest,
            use_opentofu,
            needs_validators,
        )
        .expect("step");
        let StepKind::Shell { run, env } = &step.kind else {
            panic!("prepare must be a shell step");
        };
        for tool in [
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
        ] {
            assert_eq!(
                run.contains(&catalog.tool_spec(tool)),
                needs_validators,
                "trio installs only for validator-spawning suites: {run:?}"
            );
        }
        let nextest = catalog.tool_spec(PinnedTool::Nextest);
        assert_eq!(run.contains(&nextest), use_nextest);
        let mbx = catalog.tool_spec(PinnedTool::MrBoxington);
        assert!(
            !run.contains(&mbx),
            "MBX stays action-owned for use_mbx={use_mbx}: {run:?}"
        );
        let opentofu = catalog.tool_spec(PinnedTool::Opentofu);
        assert_eq!(run.contains(&opentofu), use_opentofu);
        let rust = catalog.tool_spec(PinnedTool::Rust);
        assert_eq!(
            run.contains(&rust),
            use_rust,
            "rust installs only for rust roles: {run:?}"
        );
        for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
            assert_eq!(
                env.contains_key(key),
                use_rust,
                "triple follows the rust role: {env:?}"
            );
        }
        assert_eq!(declared_config_variables(), [] as [String; 0]);
    }
}

#[test]
fn crate_tools_install_exact_pinned_set() {
    use velnor_actions_mise::PinnedTool;

    use crate::matrix_step::prepare_crate_tools_step;
    let catalog = ToolCatalog::pinned();
    for needs_validators in [false, true] {
        for (use_rust, use_mbx, use_nextest, use_opentofu) in [
            (true, false, false, false),
            (true, false, true, true),
            (true, true, false, true),
            (true, true, true, false),
            (false, false, false, true),
        ] {
            let step = prepare_crate_tools_step(
                &catalog,
                use_rust,
                use_nextest,
                use_opentofu,
                needs_validators,
            )
            .expect("step");
            let StepKind::Shell { run, .. } = &step.kind else {
                panic!("prepare must be a shell step");
            };
            let at = run
                .iter()
                .position(|arg| arg == "install")
                .expect("install argv");
            let specs = &run[at + 1..];
            let mut expected = Vec::new();
            if use_rust {
                expected.push(catalog.tool_spec(PinnedTool::Rust));
            }
            if use_opentofu {
                expected.push(catalog.tool_spec(PinnedTool::Opentofu));
            }
            if needs_validators {
                expected.extend([
                    catalog.tool_spec(PinnedTool::Actionlint),
                    catalog.tool_spec(PinnedTool::Shellcheck),
                    catalog.tool_spec(PinnedTool::Zizmor),
                ]);
            }
            if use_nextest {
                expected.push(catalog.tool_spec(PinnedTool::Nextest));
            }
            assert_eq!(
                specs,
                expected.as_slice(),
                "exact crate install set (rust={use_rust}, mbx-action-selected={use_mbx}, nextest={use_nextest}, opentofu={use_opentofu}, validators={needs_validators})"
            );
            for absent in [PinnedTool::Gh, PinnedTool::ReleasePlz] {
                assert!(
                    !specs.contains(&catalog.tool_spec(absent)),
                    "crate jobs never install {absent:?}: {specs:?}"
                );
            }
        }
    }
}

#[test]
fn crate_tools_install_the_repo_mr_boxington_pin_only_for_the_wrapper_option() {
    use velnor_actions_mise::PinnedTool;

    use crate::matrix_step::prepare_crate_tools_step;
    let flag_catalog = ToolCatalog::pinned().with_rust_mr_boxington(true);
    let pinned_catalog = ToolCatalog::pinned();
    for needs_validators in [false, true] {
        for use_nextest in [false, true] {
            let step =
                prepare_crate_tools_step(&flag_catalog, true, use_nextest, false, needs_validators)
                    .expect("step");
            let StepKind::Shell { run, env } = &step.kind else {
                panic!("prepare must be a shell step");
            };
            let at = run
                .iter()
                .position(|arg| arg == "install")
                .expect("install argv");
            let specs = &run[at + 1..];
            let mut expected = vec![
                flag_catalog.tool_spec(PinnedTool::Rust),
                flag_catalog.tool_spec(PinnedTool::MrBoxington),
            ];
            if needs_validators {
                expected.extend([
                    flag_catalog.tool_spec(PinnedTool::Actionlint),
                    flag_catalog.tool_spec(PinnedTool::Shellcheck),
                    flag_catalog.tool_spec(PinnedTool::Zizmor),
                ]);
            }
            if use_nextest {
                expected.push(flag_catalog.tool_spec(PinnedTool::Nextest));
            }
            assert_eq!(
                specs,
                expected.as_slice(),
                "the repo mr-boxington pin joins the drivers (validators={needs_validators}, nextest={use_nextest})"
            );
            for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
                assert!(env.contains_key(key), "rust role keeps the triple: {env:?}");
            }
            let plain = prepare_crate_tools_step(
                &pinned_catalog,
                true,
                use_nextest,
                false,
                needs_validators,
            )
            .expect("step");
            let StepKind::Shell { run: plain_run, .. } = &plain.kind else {
                panic!("prepare must be a shell step");
            };
            assert!(
                !plain_run.contains(&flag_catalog.tool_spec(PinnedTool::MrBoxington)),
                "without the wrapper option mr-boxington stays action-owned"
            );
        }
    }
}

#[test]
fn pure_tofu_crate_jobs_never_install_the_repo_mr_boxington_pin() {
    use velnor_actions_mise::PinnedTool;

    use crate::matrix_step::prepare_crate_tools_step;
    let flag_catalog = ToolCatalog::pinned().with_rust_mr_boxington(true);
    let mbx_spec = flag_catalog.tool_spec(PinnedTool::MrBoxington);
    for needs_validators in [false, true] {
        for use_nextest in [false, true] {
            let step =
                prepare_crate_tools_step(&flag_catalog, false, use_nextest, true, needs_validators)
                    .expect("step");
            let StepKind::Shell { run, env } = &step.kind else {
                panic!("prepare must be a shell step");
            };
            assert!(
                !run.contains(&mbx_spec),
                "a job that runs no cargo must not install the repo pin (validators={needs_validators}, nextest={use_nextest}): {run:?}"
            );
            for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
                assert!(
                    !env.contains_key(key),
                    "pure-tofu roles keep no owned-homes triple: {env:?}"
                );
            }
        }
    }
}
