// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use velnor_actions_contract::workflow::cache_family::COMPLETE_STATE_TRANSPORT_EXPR;
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_EXPORT_CONDITION, MBX_BUNDLE_IMPORT_CONDITION, MBX_BUNDLE_KEY_EXPR,
    MBX_BUNDLE_PATH_EXPR, MBX_BUNDLE_RESTORE_KEYS_EXPR, MBX_BUNDLE_SAVE_CONDITION,
};
use velnor_actions_contract::{Job, JobTimeout, Step, StepKind, StepRole};
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, MBX_BUNDLE_IMPORT_NAME, MBX_BUNDLE_KEY_NAME, MBX_BUNDLE_RESTORE_NAME,
    MbxTransport, TOOLS_RESTORE_ACTION_USES, TOOLS_SAVE_USES, cache_action_step, check_mbx_gating,
    mbx_producer_mark_step, mbx_steps_for_driver,
};
use velnor_actions_workflow_renderer::toolchain_env::checked_task_env;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn driver_env() -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    Ok(checked_task_env(
        &BTreeMap::new(),
        "${{ runner.temp }}/velnor/rustup",
        "${{ runner.temp }}/velnor/cargo",
        "1.89.0",
    )?)
}

fn mbx_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn transport_steps() -> Result<Vec<Step>, Box<dyn Error>> {
    let steps = mbx_steps_for_driver(
        &mbx_uses(),
        CompileDriver::Mbx,
        "1.2.3",
        "1.89.0",
        driver_env()?,
        MbxTransport::SeedRead,
        None,
    )?
    .ok_or_else(|| fail("mbx driver produced no steps"))?;
    if steps.len() != 6 {
        return Err(fail("mbx driver lost its bundle prelude"));
    }
    Ok(steps)
}

fn shell_script(step: &Step) -> Result<&str, Box<dyn Error>> {
    match &step.kind {
        StepKind::Shell { run, .. } => run
            .get(2)
            .map(String::as_str)
            .ok_or_else(|| fail("shell step left its inline script position")),
        _ => Err(fail("expected a shell step")),
    }
}

fn action_with(step: &Step) -> Result<&BTreeMap<String, String>, Box<dyn Error>> {
    match &step.kind {
        StepKind::Action { with, .. } => Ok(with),
        _ => Err(fail("expected an action step")),
    }
}

fn check_shell_syntax(step: &Step, label: &str) -> Result<(), Box<dyn Error>> {
    let script = shell_script(step)?;
    let output = std::process::Command::new("sh")
        .arg("-n")
        .arg("-c")
        .arg(script)
        .output()
        .map_err(|error| fail(&format!("shell check failed to run: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(fail(&format!(
            "{label} left invalid shell: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

const MATCHED_KEY_BAKE: &str = "${{steps.mbx-bundle-restore.outputs.cache-matched-key}}";

fn job_with(steps: Vec<Step>) -> Job {
    Job {
        display_name: "gc1-t30".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        steps,
    }
}

fn setup_step(with: BTreeMap<String, String>) -> Step {
    Step {
        name: "Setup".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: format!(
                "jdx/mise-action@{}",
                velnor_actions_actionlint::actions::MISE_ACTION_SHA
            ),
            with,
            env: BTreeMap::new(),
        },
    }
}

#[path = "gc1_t30_behave.rs"]
mod gc1_t30_behave;
#[path = "gc1_t30_checks.rs"]
mod gc1_t30_checks;
#[path = "gc1_t30_reject.rs"]
mod gc1_t30_reject;
