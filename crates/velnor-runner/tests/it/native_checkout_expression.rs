// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, finish};
use crate::native_checkout_inputs::MARKER_STEP;

const REFUSAL: &str =
    "actions/checkout cannot validate an expression-valued 'with:' in velnor-runner";

fn with_inputs(job: &Path, inputs: &Value) -> Result<()> {
    let mut message: Value = serde_json::from_str(&std::fs::read_to_string(job)?)?;
    let steps = message["Steps"]
        .as_array_mut()
        .context("the job has steps")?;
    let checkout = steps
        .iter_mut()
        .find(|step| step["Reference"]["Name"] == "actions/checkout")
        .context("the job has a checkout step")?;
    checkout["Inputs"] = inputs.clone();
    std::fs::write(job, serde_json::to_string(&message)?)?;
    Ok(())
}

fn assert_refused_before_step_one(rig: &Rig, inputs: &Value) -> Result<()> {
    let job = rig.job_edit(
        "checkout_depth1",
        &rig.head,
        &rig.server_url(),
        &[("\"Steps\": [", MARKER_STEP)],
    )?;
    with_inputs(&job, inputs)?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.contains(REFUSAL),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!ran.stdout.contains("step-one-marker"), "step 1 ran");
    assert!(!rig.trace().exists(), "git ran");
    Ok(())
}

#[test]
fn an_expression_valued_with_refuses_the_job_before_its_first_step() -> Result<()> {
    assert_refused_before_step_one(
        &Rig::new()?,
        &json!({"type": 3, "expr": "fromJSON('{\"persist-credentials\":\"false\"}')"}),
    )
}

#[test]
fn an_expression_input_name_refuses_the_job_before_its_first_step() -> Result<()> {
    assert_refused_before_step_one(
        &Rig::new()?,
        &json!({"type": 2, "map": [{
            "key": {"type": 3, "expr": "'persist-credentials'"},
            "value": {"type": 0, "lit": "false"}
        }]}),
    )
}
