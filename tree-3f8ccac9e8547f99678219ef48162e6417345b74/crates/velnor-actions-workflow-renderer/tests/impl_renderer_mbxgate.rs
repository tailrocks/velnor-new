//! MBX action gating: emitted only for MBX-selected drivers.
use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::{
    CompileDriver, RenderError, check_mbx_gating, checkout_step, mbx_steps_for_driver, shell_step,
};

use super::impl_renderer_fixtures::*;

fn mbx_pin() -> String {
    format!("jdx/mr-boxington-action@{:040x}", 0)
}

fn mbx_argv() -> Vec<String> {
    mise_argv("mr-boxington@0.9.7", "mbx", &["--version"])
}

#[test]
fn mbx_emitted_only_for_mbx_driver() -> Result<(), RenderError> {
    let [preflight, selected] = mbx_tool_steps(&mbx_pin(), "1.19.0", "1.98.1")?;
    assert_eq!(preflight.name, "Verify MBX and Rust toolchains");
    assert_eq!(selected.name, "Restore MBX objects");
    assert!(
        mbx_steps_for_driver(
            &mbx_pin(),
            CompileDriver::Cargo,
            "1.19.0",
            "1.98.1",
            mbx_tool_env("1.98.1"),
        )?
        .is_none()
    );
    assert!(
        mbx_steps_for_driver(
            &checkout_pin(),
            CompileDriver::Mbx,
            "1.19.0",
            "1.98.1",
            mbx_tool_env("1.98.1"),
        )
        .is_err_and(|err| format!("{err:?}").contains("not_mbx_action")),
        "non-MBX action must fail"
    );
    assert!(
        mbx_steps_for_driver(
            &mbx_pin(),
            CompileDriver::Mbx,
            "latest",
            "1.98.1",
            mbx_tool_env("1.98.1"),
        )
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
    let cargo_only = BTreeMap::from([("velnor-cargo".to_owned(), CompileDriver::Cargo)]);
    assert!(
        check_mbx_gating(&jobs, &cargo_only)
            .is_err_and(|err| format!("{err:?}").contains("mbx_tool_without_selection")),
        "mbx tool on cargo leg must fail"
    );
    jobs.get_mut("velnor-cargo").expect("cargo job").steps.pop();
    check_mbx_gating(&jobs, &cargo_only)?;
    let mbx = mbx_tool_steps(&mbx_pin(), "1.19.0", "1.98.1")?;
    jobs.get_mut("velnor-cargo")
        .expect("cargo job")
        .steps
        .extend(mbx);
    assert!(
        check_mbx_gating(&jobs, &cargo_only)
            .is_err_and(|err| format!("{err:?}").contains("mbx_action_without_selection")),
        "mbx action on cargo leg must fail"
    );
    let mbx_only = BTreeMap::from([("velnor-cargo".to_owned(), CompileDriver::Mbx)]);
    check_mbx_gating(&jobs, &mbx_only)?;
    jobs.get_mut("velnor-cargo").expect("cargo job").steps.pop();
    assert!(
        check_mbx_gating(&jobs, &mbx_only)
            .is_err_and(|err| format!("{err:?}").contains("mbx_missing_for_selection")),
        "mbx leg without action must fail"
    );
    let unknown = BTreeMap::from([("velnor-absent".to_owned(), CompileDriver::Mbx)]);
    assert!(
        check_mbx_gating(&jobs, &unknown)
            .is_err_and(|err| format!("{err:?}").contains("mbx_gating_unknown_job")),
        "driver for absent job must fail"
    );
    check_mbx_gating(&jobs, &BTreeMap::new())?;
    Ok(())
}
