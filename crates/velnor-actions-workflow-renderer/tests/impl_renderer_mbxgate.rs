//! MBX action gating: emitted only for MBX-selected drivers.
use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::{
    CompilerDriver, RenderError, check_mbx_gating, checkout_step, mbx_step_for_driver, shell_step,
};

use super::impl_renderer_fixtures::*;

fn gate(
    jobs: &BTreeMap<String, velnor_actions_contract::Job>,
    drivers: &BTreeMap<String, CompilerDriver>,
) -> Result<(), RenderError> {
    check_mbx_gating(jobs, drivers, &[])
}

fn mbx_pin() -> String {
    format!("jdx/mr-boxington-action@{:040x}", 0)
}

fn mbx_argv() -> Vec<String> {
    mise_argv("mr-boxington@0.9.7", "mbx", &["--version"])
}

#[test]
fn mbx_emitted_only_for_mbx_driver() -> Result<(), RenderError> {
    let selected =
        mbx_step_for_driver(&mbx_pin(), CompilerDriver::Mbx, "1.19.0")?.expect("mbx step");
    assert_eq!(selected.name, "Restore MBX objects");
    assert!(mbx_step_for_driver(&mbx_pin(), CompilerDriver::Cargo, "1.19.0")?.is_none());
    assert!(
        mbx_step_for_driver(&checkout_pin(), CompilerDriver::Mbx, "1.19.0")
            .is_err_and(|err| format!("{err:?}").contains("not_mbx_action")),
        "non-MBX action must fail"
    );
    assert!(
        mbx_step_for_driver(&mbx_pin(), CompilerDriver::Mbx, "latest")
            .is_err_and(|err| format!("{err:?}").contains("bad_mbx_version")),
        "floating action version must fail"
    );
    Ok(())
}

#[test]
fn mbx_gating_rejects_unselected_mbx() -> Result<(), RenderError> {
    let cargo = job(
        "velnor-cargo",
        "Cargo leg",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            shell_step("Run task", mbx_argv(), BTreeMap::new())?,
        ],
    );
    let mut jobs = BTreeMap::from([cargo]);
    let cargo_only = BTreeMap::from([("velnor-cargo".to_owned(), CompilerDriver::Cargo)]);
    assert!(
        gate(&jobs, &cargo_only)
            .is_err_and(|err| format!("{err:?}").contains("mbx_tool_without_selection")),
        "mbx tool on cargo leg must fail"
    );
    jobs.get_mut("velnor-cargo").expect("cargo job").steps.pop();
    gate(&jobs, &cargo_only)?;
    let mbx = mbx_step_for_driver(&mbx_pin(), CompilerDriver::Mbx, "1.19.0")?.expect("mbx step");
    jobs.get_mut("velnor-cargo")
        .expect("cargo job")
        .steps
        .push(mbx);
    assert!(
        gate(&jobs, &cargo_only)
            .is_err_and(|err| format!("{err:?}").contains("mbx_action_without_selection")),
        "mbx action on cargo leg must fail"
    );
    let mbx_only = BTreeMap::from([("velnor-cargo".to_owned(), CompilerDriver::Mbx)]);
    assert!(
        gate(&jobs, &mbx_only)
            .is_err_and(|err| format!("{err:?}").contains("mbx_missing_for_selection")),
        "objects transport cannot replace selected compiler execution"
    );
    jobs.get_mut("velnor-cargo")
        .expect("cargo job")
        .steps
        .insert(1, shell_step("Run task", mbx_argv(), BTreeMap::new())?);
    assert!(
        gate(&jobs, &mbx_only)
            .is_err_and(|error| format!("{error:?}").contains("mbx_missing_for_selection"))
    );
    jobs.get_mut("velnor-cargo").expect("cargo job").steps.pop();
    assert!(
        gate(&jobs, &mbx_only)
            .is_err_and(|error| format!("{error:?}").contains("mbx_missing_for_selection"))
    );
    jobs.get_mut("velnor-cargo").expect("cargo job").steps.pop();
    assert!(
        gate(&jobs, &mbx_only)
            .is_err_and(|err| format!("{err:?}").contains("mbx_missing_for_selection")),
        "mbx leg without compiler execution must fail"
    );
    let unknown = BTreeMap::from([("velnor-absent".to_owned(), CompilerDriver::Mbx)]);
    assert!(
        gate(&jobs, &unknown)
            .is_err_and(|err| format!("{err:?}").contains("mbx_gating_unknown_job")),
        "driver for absent job must fail"
    );
    gate(&jobs, &BTreeMap::new())?;
    Ok(())
}

#[test]
fn mbx_gating_separates_execution_from_optional_transport() -> Result<(), RenderError> {
    let action =
        mbx_step_for_driver(&mbx_pin(), CompilerDriver::Mbx, "1.19.0")?.expect("objects transport");
    let execute = shell_step("Run task", mbx_argv(), BTreeMap::new())?;
    let drivers = BTreeMap::from([("selected".to_owned(), CompilerDriver::Mbx)]);
    for actions in 0..=2 {
        let mut steps = vec![execute.clone(), execute.clone()];
        steps.extend(std::iter::repeat_n(action.clone(), actions));
        let jobs = BTreeMap::from([job("selected", "MBX", Vec::new(), steps)]);
        if actions <= 1 {
            assert!(
                gate(&jobs, &drivers)
                    .is_err_and(|error| format!("{error:?}").contains("mbx_missing_for_selection"))
            );
        } else {
            assert!(
                gate(&jobs, &drivers)
                    .is_err_and(|error| format!("{error:?}").contains("mbx_duplicated"))
            );
        }
    }
    let install = shell_step(
        "Install tool",
        vec![
            "mise".to_owned(),
            "install".to_owned(),
            "mr-boxington@1.19.0".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    let jobs = BTreeMap::from([job("selected", "MBX", Vec::new(), vec![install])]);
    assert!(
        gate(&jobs, &drivers)
            .is_err_and(|error| format!("{error:?}").contains("mbx_missing_for_selection"))
    );
    Ok(())
}

#[test]
fn mbx_gating_rejects_unqualified_inline_compiler_execution() -> Result<(), RenderError> {
    let execute = shell_step(
        "Run wrapped task",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "mise --no-config exec mr-boxington@1.19.0 -- mbx test --locked".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    let jobs = BTreeMap::from([job("selected", "MBX", Vec::new(), vec![execute])]);
    assert!(
        gate(
            &jobs,
            &BTreeMap::from([("selected".to_owned(), CompilerDriver::Mbx)])
        )
        .is_err_and(|error| format!("{error:?}").contains("mbx_missing_for_selection"))
    );
    Ok(())
}

#[test]
fn mbx_arguments_and_paths_do_not_select_a_compiler() -> Result<(), RenderError> {
    let negatives = [
        vec!["printf", "%s", "mbx"],
        vec!["cargo", "test", "--package", "mbx"],
        vec!["cargo", "test", "--package", "mr-boxington"],
        vec!["cat", "some/mr-boxington/file"],
        vec![
            "mise",
            "exec",
            "rust@1.98.1",
            "--",
            "cargo",
            "test",
            "--package",
            "mbx",
        ],
        vec!["mise", "exec", "rust@1.98.1", "--", "printf", "%s", "mbx"],
        vec!["sh", "-c", "printf %s mbx"],
        vec!["mise", "install", "other/mr-boxington@1.19.0"],
    ];
    for argv in negatives {
        let step = shell_step(
            "Non MBX",
            argv.into_iter().map(str::to_owned).collect(),
            BTreeMap::new(),
        )?;
        let jobs = BTreeMap::from([job("selected", "MBX", Vec::new(), vec![step])]);
        gate(
            &jobs,
            &BTreeMap::from([("selected".to_owned(), CompilerDriver::Cargo)]),
        )?;
        assert!(
            gate(
                &jobs,
                &BTreeMap::from([("selected".to_owned(), CompilerDriver::Mbx)])
            )
            .is_err_and(|error| format!("{error:?}").contains("mbx_missing_for_selection"))
        );
    }
    Ok(())
}

#[test]
fn cargo_rejects_exact_compiled_mbx_installation_footprint() -> Result<(), RenderError> {
    use velnor_actions_contract::{
        HelperInvocation, SourceBoundHelper, SourceBoundOperation, Step, StepKind,
    };
    let operation = SourceBoundOperation::RustPrepareRootLinux;
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &"0".repeat(64))
        .expect("shape only; installation is not compiler authority");
    for selector in [
        "mr-boxington@1.19.0",
        "github:jdx/mr-boxington@1.19.0",
        "other/mr-boxington@1.19.0",
        "tool@mr-boxington",
    ] {
        let invocation =
            HelperInvocation::compiled(descriptor.clone(), Vec::new(), vec![selector.to_owned()])
                .expect("installation footprint shape");
        let install = Step {
            id: None,
            name: "Prepare tools".to_owned(),
            condition: None,
            kind: StepKind::SourceBoundHelper {
                invocation,
                env: BTreeMap::new(),
            },
        };
        let jobs = BTreeMap::from([job("selected", "Cargo", Vec::new(), vec![install])]);
        let cargo = BTreeMap::from([("selected".to_owned(), CompilerDriver::Cargo)]);
        if selector.starts_with("mr-boxington@") || selector.starts_with("github:jdx/mr-boxington@")
        {
            assert!(gate(&jobs, &cargo).is_err_and(|error|
                format!("{error:?}").contains("mbx_tool_without_selection")));
        } else {
            gate(&jobs, &cargo)?;
        }
        assert!(
            gate(
                &jobs,
                &BTreeMap::from([("selected".to_owned(), CompilerDriver::Mbx)])
            )
            .is_err_and(|error| format!("{error:?}").contains("mbx_missing_for_selection"))
        );
    }
    Ok(())
}
