//! Token hygiene: prelude recognition in mixed-trust scripts.
//!
//! Split from `impl_renderer_token_hygiene` (size gate): the scanner
//! strips the exact credential-unset prelude in command position.

use std::collections::BTreeMap;

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::StepRole;
use velnor_actions_workflow_renderer::{
    RenderError, ambient_shell_step, checkout_step, plan_step, render_workflow_ir,
    toolchain_env::credential_unset_prelude,
};

use super::impl_renderer_fixtures::*;

#[test]
fn token_hygiene_strips_midscript_prelude_command() -> Result<(), RenderError> {
    // A mixed-trust validator step (ambient install, scrubbed payload)
    // carries the exact prelude after `&&`: the protection strips in
    // command position, so the render succeeds.
    let script = format!(
        "{{ mise install 'tool@1.0.0' && {} }} && true",
        credential_unset_prelude()
    );
    let mut deny = ambient_shell_step(
        "Renamed validator display",
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )?;
    deny.role = Some(StepRole::CargoDeny);
    let mixed = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, deny, plan_step()],
    );
    render_workflow_ir(
        &fixture_ir(vec![mixed]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    Ok(())
}

#[test]
fn token_hygiene_rejects_doubled_prelude() -> Result<(), RenderError> {
    // Only the first command-unit prelude strips: a doubled prelude
    // (caller preluded a script the constructor preludes again) still
    // trips fail-closed.
    let prelude = credential_unset_prelude();
    let doubled = token_plan_job(
        "Task",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("{prelude} {prelude} true"),
        ],
        BTreeMap::new(),
    )?;
    render_fails_with(vec![doubled], "token_in_run");
    Ok(())
}

#[test]
fn token_hygiene_rejects_quoted_prelude_echo() -> Result<(), RenderError> {
    // Quoting the prelude prints token names instead of unsetting them:
    // not a command unit, so the scan still flags it.
    let echoed = token_plan_job(
        "Task",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("echo \"{}\"", credential_unset_prelude()),
        ],
        BTreeMap::new(),
    )?;
    render_fails_with(vec![echoed], "token_in_run");
    Ok(())
}
