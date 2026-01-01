//! External Mise selection cannot install or execute MBX outside its action.
use std::collections::BTreeMap;

use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::{
    CompileDriver, RenderError, check_mbx_gating, mise_setup_step, shell_step,
};

use super::impl_renderer_fixtures::*;

fn selector_error(run: Vec<String>, driver: Option<CompileDriver>) -> Result<(), RenderError> {
    let step = shell_step("External tool selection", run, BTreeMap::new())?;
    let jobs = BTreeMap::from([job("selector-job", "Selector job", Vec::new(), vec![step])]);
    let drivers = driver.map_or_else(BTreeMap::new, |driver| {
        BTreeMap::from([("selector-job".to_owned(), driver)])
    });
    assert!(
        check_mbx_gating(&jobs, &drivers)
            .is_err_and(|error| format!("{error:?}").contains("mbx_mise_selector_forbidden")),
        "an external MBX selector must fail regardless of the selected driver"
    );
    Ok(())
}

fn mise_exec(specs: &[&str], payload: &[&str]) -> Vec<String> {
    [
        &["mise", "--no-config", "--no-env", "--no-hooks", "exec"][..],
        specs,
        &["--"][..],
        payload,
    ]
    .concat()
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[test]
fn duplicate_mbx_selector_in_exec_fails_for_both_drivers() -> Result<(), RenderError> {
    for payload in ["cargo", "mbx"] {
        let run = mise_exec(&["rust@1.98.1", "mr-boxington@1.21.1"], &[payload, "build"]);
        selector_error(run.clone(), Some(CompileDriver::Cargo))?;
        selector_error(run, Some(CompileDriver::Mbx))?;
    }
    Ok(())
}

#[test]
fn external_install_is_rejected_but_mise_decoy_text_is_not() -> Result<(), RenderError> {
    selector_error(
        vec![
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            "rust@1.98.1",
            "mr-boxington@1.21.1",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        None,
    )?;
    for literal in [";", "&&", "||", "|", "then", "if"] {
        let script = format!(
            "printf '%s\\n' '{literal}' mise --no-config --no-env --no-hooks install mr-boxington@1.21.1"
        );
        let decoy = shell_step(
            "Print a command example",
            vec!["sh".to_owned(), "-c".to_owned(), script],
            BTreeMap::new(),
        )?;
        let jobs = BTreeMap::from([job("decoy", "Decoy", Vec::new(), vec![decoy])]);
        check_mbx_gating(&jobs, &BTreeMap::new())?;
    }
    let escaped = shell_step(
        "Print an escaped command example",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "printf '%s\\n' safe\\;\\ mise --no-config --no-env --no-hooks install mr-boxington@1.21.1".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    let jobs = BTreeMap::from([job("escaped", "Escaped", Vec::new(), vec![escaped])]);
    check_mbx_gating(&jobs, &BTreeMap::new())
}

#[test]
fn malformed_inline_quote_fails_closed_for_selector_scanning() -> Result<(), RenderError> {
    let step = shell_step(
        "Unterminated diagnostic",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "printf '%s\\n' 'mise --no-config --no-env --no-hooks install mr-boxington@1.21.1"
                .to_owned(),
        ],
        BTreeMap::new(),
    )?;
    let jobs = BTreeMap::from([job("malformed", "Malformed", Vec::new(), vec![step])]);
    assert!(
        check_mbx_gating(&jobs, &BTreeMap::new())
            .is_err_and(|error| format!("{error:?}").contains("mbx_mise_selector_forbidden")),
        "an unterminated quote leaves command boundaries unknown"
    );
    Ok(())
}

#[test]
fn mise_setup_is_admitted_only_when_it_does_not_install_project_tools() -> Result<(), RenderError> {
    let setup = mise_setup_step(&mise())?;
    let mut job = job("setup", "Mise setup", Vec::new(), vec![setup]);
    check_mbx_gating(&BTreeMap::from([job.clone()]), &BTreeMap::new())?;
    let StepKind::Action { with, .. } = &mut job.1.steps[0].kind else {
        return Err(RenderError::InvalidWorkflow(
            "expected_mise_action".to_owned(),
        ));
    };
    with.insert("install".to_owned(), "true".to_owned());
    let jobs = BTreeMap::from([job.clone()]);
    assert!(
        check_mbx_gating(&jobs, &BTreeMap::new())
            .is_err_and(|error| format!("{error:?}").contains("mbx_mise_selector_forbidden")),
        "a Mise action that installs project tools can install MBX outside the cache owner"
    );
    let StepKind::Action { with, .. } = &mut job.1.steps[0].kind else {
        return Err(RenderError::InvalidWorkflow(
            "expected_mise_action".to_owned(),
        ));
    };
    with.remove("install");
    assert!(
        check_mbx_gating(&BTreeMap::from([job]), &BTreeMap::new())
            .is_err_and(|error| format!("{error:?}").contains("mbx_mise_selector_forbidden")),
        "an implicit Mise installer cannot be admitted"
    );
    Ok(())
}

#[test]
fn mise_action_tool_declaration_cannot_select_mbx() -> Result<(), RenderError> {
    let setup = mise_setup_step(&mise())?;
    let mut job = job("setup", "Mise setup", Vec::new(), vec![setup]);
    let StepKind::Action { with, .. } = &mut job.1.steps[0].kind else {
        return Err(RenderError::InvalidWorkflow(
            "expected_mise_action".to_owned(),
        ));
    };
    with.insert(
        "tool_versions".to_owned(),
        "rust@1.98.1 mr-boxington@1.21.1".to_owned(),
    );
    assert!(
        check_mbx_gating(&BTreeMap::from([job]), &BTreeMap::new())
            .is_err_and(|error| format!("{error:?}").contains("mbx_mise_selector_forbidden")),
        "typed setup cannot smuggle an MBX selector through tool declarations"
    );
    Ok(())
}

#[test]
fn rust_nextest_exec_remains_outside_the_mbx_selector_gate() -> Result<(), RenderError> {
    let step = shell_step(
        "Run tests",
        mise_exec(
            &["rust@1.98.1", "cargo-nextest@0.9.148"],
            &["cargo", "nextest", "run", "--locked"],
        ),
        BTreeMap::new(),
    )?;
    let jobs = BTreeMap::from([job("tests", "Tests", Vec::new(), vec![step])]);
    check_mbx_gating(&jobs, &BTreeMap::new())
}
