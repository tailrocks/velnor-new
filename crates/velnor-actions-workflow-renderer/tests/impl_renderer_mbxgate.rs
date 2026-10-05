//! MBX action gating: emitted only for MBX-selected drivers.
use std::collections::BTreeMap;
use velnor_actions_contract::StepRole;
use velnor_actions_workflow_renderer::{
    CompileDriver, RenderError, check_mbx_gating, checkout_step, mbx_steps_for_driver, shell_step,
};

use super::impl_renderer_fixtures::*;

fn mbx_pin() -> String {
    format!("jdx/mr-boxington-action@{:040x}", 0)
}

fn mbx_argv() -> Vec<String> {
    vec!["mbx".to_owned(), "--version".to_owned()]
}

fn report_wrapped_mbx_task() -> Result<velnor_actions_contract::Step, RenderError> {
    let script = concat!(
        "s=$(date +%s%3N); ",
        "mise --no-config --no-env --no-hooks exec rust@1.98.1 -- mbx test; ",
        "code=$?; VELNOR_EXIT_CODE=\"$code\" VELNOR_START_MS=\"$s\" ",
        "VELNOR_INTERNAL_OP=write-task-report-v1 \"$RUNNER_TEMP/velnor/bin/velnor-actions\"; ",
        "helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""
    );
    shell_step(
        "MBX task label",
        vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
        BTreeMap::new(),
    )
}

#[test]
fn mbx_emitted_only_for_mbx_driver() -> Result<(), RenderError> {
    let [preflight, selected, version_check] = mbx_tool_steps(&mbx_pin(), "1.19.0", "1.98.1")?;
    assert_eq!(preflight.name, "Verify Rust before MBX action");
    assert_eq!(selected.name, "Restore MBX objects");
    assert_eq!(version_check.name, "Verify native MBX version");
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
fn cargo_gating_rejects_mbx_commands_but_ignores_decoy_text() -> Result<(), RenderError> {
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
    jobs.get_mut("velnor-cargo")
        .expect("cargo job")
        .steps
        .push(shell_step(
            "Name mentions mr-boxington",
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "printf '%s\\n' 'mbx mr-boxington'".to_owned(),
            ],
            BTreeMap::new(),
        )?);
    check_mbx_gating(&jobs, &cargo_only)?;
    Ok(())
}

#[test]
fn cargo_gating_rejects_the_native_action() -> Result<(), RenderError> {
    let mut steps = vec![checkout_step(&checkout_pin())?];
    steps.extend(mbx_tool_steps(&mbx_pin(), "1.19.0", "1.98.1")?);
    let jobs = BTreeMap::from([job("velnor-cargo", "Cargo leg", Vec::new(), steps)]);
    let cargo = BTreeMap::from([("velnor-cargo".to_owned(), CompileDriver::Cargo)]);
    assert!(
        check_mbx_gating(&jobs, &cargo)
            .is_err_and(|err| format!("{err:?}").contains("mbx_action_without_selection")),
        "MBX action on cargo leg must fail"
    );
    Ok(())
}

#[test]
fn mbx_gating_requires_typed_action_and_preflight_roles() -> Result<(), RenderError> {
    let mut steps = vec![checkout_step(&checkout_pin())?];
    steps.extend(mbx_tool_steps(&mbx_pin(), "1.19.0", "1.98.1")?);
    let mut jobs = BTreeMap::from([job("velnor-mbx", "MBX leg", Vec::new(), steps)]);
    let mbx = BTreeMap::from([("velnor-mbx".to_owned(), CompileDriver::Mbx)]);
    let preflight = jobs.get("velnor-mbx").expect("MBX job").steps[1].clone();
    jobs.get_mut("velnor-mbx").expect("MBX job").steps.remove(1);
    assert!(
        check_mbx_gating(&jobs, &mbx)
            .is_err_and(|err| format!("{err:?}").contains("mbx_preflight_missing")),
        "an MBX action requires its typed Rust preflight"
    );
    jobs.get_mut("velnor-mbx")
        .expect("MBX job")
        .steps
        .insert(1, preflight);
    jobs.get_mut("velnor-mbx").expect("MBX job").steps[2].role = None;
    assert!(
        check_mbx_gating(&jobs, &mbx)
            .is_err_and(|err| format!("{err:?}").contains("mbx_action_role_missing")),
        "an MBX action needs its typed owner role"
    );
    Ok(())
}

#[test]
fn mbx_gating_requires_the_exact_guard_before_a_real_command() -> Result<(), RenderError> {
    let mut steps = vec![checkout_step(&checkout_pin())?];
    steps.extend(mbx_tool_steps(&mbx_pin(), "1.19.0", "1.98.1")?);
    let mut jobs = BTreeMap::from([job("velnor-mbx", "MBX leg", Vec::new(), steps)]);
    let mbx = BTreeMap::from([("velnor-mbx".to_owned(), CompileDriver::Mbx)]);
    jobs.get_mut("velnor-mbx").expect("MBX job").steps[2].role = Some(StepRole::MbxCache);
    jobs.get_mut("velnor-mbx")
        .expect("MBX job")
        .steps
        .push(shell_step(
            "Name mentions MBX",
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "printf '%s\\n' 'mbx mr-boxington'".to_owned(),
            ],
            BTreeMap::new(),
        )?);
    assert!(
        check_mbx_gating(&jobs, &mbx)
            .is_err_and(|err| format!("{err:?}").contains("mbx_version_check_order")),
        "step names and incidental text do not satisfy the MBX command gate"
    );
    jobs.get_mut("velnor-mbx").expect("MBX job").steps.pop();
    jobs.get_mut("velnor-mbx")
        .expect("MBX job")
        .steps
        .push(report_wrapped_mbx_task()?);
    check_mbx_gating(&jobs, &mbx)?;

    jobs.get_mut("velnor-mbx").expect("MBX job").steps[3].condition = Some("always()".to_owned());
    assert!(
        check_mbx_gating(&jobs, &mbx)
            .is_err_and(|err| format!("{err:?}").contains("mbx_version_check_mismatch")),
        "the exact PATH check cannot be replaced with a conditional step"
    );
    jobs.get_mut("velnor-mbx").expect("MBX job").steps[3].condition = None;
    jobs.get_mut("velnor-mbx")
        .expect("MBX job")
        .steps
        .swap(3, 4);
    assert!(
        check_mbx_gating(&jobs, &mbx)
            .is_err_and(|err| format!("{err:?}").contains("mbx_version_check_order")),
        "the exact PATH version check must precede MBX commands"
    );
    jobs.get_mut("velnor-mbx")
        .expect("MBX job")
        .steps
        .swap(3, 4);
    jobs.get_mut("velnor-mbx").expect("MBX job").steps.remove(3);
    assert!(
        check_mbx_gating(&jobs, &mbx)
            .is_err_and(|err| format!("{err:?}").contains("mbx_version_check_missing")),
        "the action must have an exact-version PATH check"
    );
    jobs.get_mut("velnor-mbx").expect("MBX job").steps.insert(
        3,
        mbx_tool_steps(&mbx_pin(), "1.19.0", "1.98.1")?[2].clone(),
    );
    jobs.get_mut("velnor-mbx").expect("MBX job").steps.remove(2);
    assert!(
        check_mbx_gating(&jobs, &mbx)
            .is_err_and(|err| format!("{err:?}").contains("mbx_missing_for_selection")),
        "mbx leg without action must fail"
    );
    Ok(())
}

#[test]
fn mbx_gating_rejects_unknown_driver_jobs() -> Result<(), RenderError> {
    let cargo = job(
        "velnor-cargo",
        "Cargo leg",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?],
    );
    let jobs = BTreeMap::from([cargo]);
    let unknown = BTreeMap::from([("velnor-absent".to_owned(), CompileDriver::Mbx)]);
    assert!(
        check_mbx_gating(&jobs, &unknown)
            .is_err_and(|err| format!("{err:?}").contains("mbx_gating_unknown_job")),
        "driver for absent job must fail"
    );
    check_mbx_gating(&jobs, &BTreeMap::new())?;
    Ok(())
}
