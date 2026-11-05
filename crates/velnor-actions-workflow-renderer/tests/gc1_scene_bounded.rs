// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, MbxTransport, StateCeilings, mbx_bundle_publish_bounded,
    mbx_steps_for_driver_bounded,
};

use crate::gc1_scene::{Rendered, Scripts, fail};
use crate::gc1_t53::{driver_env, shell_script_of};

const RECOVER_INDEX: usize = 5;
const RUST_VERSION: &str = "1.89.0";

pub(crate) fn scripts_with(ceilings: StateCeilings) -> Result<Scripts, Box<dyn Error>> {
    let steps = mbx_steps_for_driver_bounded(
        &format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
        CompileDriver::Mbx,
        "1.2.3",
        RUST_VERSION,
        driver_env()?,
        MbxTransport::SeedRead(crate::gc1_seal_support::slice_of(".")?),
        None,
        ceilings,
    )?
    .ok_or_else(|| fail("mbx driver produced no steps"))?;
    let recover = &steps[RECOVER_INDEX];
    let StepKind::Shell { env, .. } = &recover.kind else {
        return Err(fail("recover step left its shell form"));
    };
    let export = mbx_bundle_publish_bounded(RUST_VERSION, driver_env()?, ceilings)?;
    Ok(Scripts {
        discover: shell_script_of(&steps[3])?,
        recover: Rendered {
            run: shell_script_of(recover)?,
            env: env.clone().into_iter().collect(),
        },
        export: shell_script_of(&export)?,
    })
}
