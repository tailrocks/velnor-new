// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, WorkflowPolicy};

pub(crate) const EMIT_GROUP_LEDGER_OPERATION: &str = "emit-group-ledger-v1";

pub(crate) const ASSEMBLE_GROUP_LEDGERS_OPERATION: &str = "assemble-group-ledgers-v1";

pub(crate) const EMIT_GROUP_LEDGER_STEP_NAME: &str = "Emit group ledger";

pub(crate) const ASSEMBLE_GROUP_LEDGERS_STEP_NAME: &str = "Assemble group ledgers";

pub(crate) const JOB_STATUS_ENV: &str = "VELNOR_JOB_STATUS";

fn cargo_run_argv() -> Vec<String> {
    vec![
        "cargo".to_owned(),
        "run".to_owned(),
        "--locked".to_owned(),
        "-p".to_owned(),
        "velnor-actions-cli".to_owned(),
        "--bin".to_owned(),
        "velnor-actions".to_owned(),
    ]
}

#[must_use]
pub fn crate_group_ledger_step(policy: WorkflowPolicy) -> Step {
    let env = crate::toolchain_env::with_credential_scrub(&BTreeMap::from([
        (
            crate::steps::INTERNAL_OP_ENV.to_owned(),
            EMIT_GROUP_LEDGER_OPERATION.to_owned(),
        ),
        (JOB_STATUS_ENV.to_owned(), "${{ job.status }}".to_owned()),
    ]));
    let kind = if policy == WorkflowPolicy::ConsumerV1 {
        let mut env = env;
        env.remove(crate::steps::INTERNAL_OP_ENV);
        StepKind::Internal {
            operation: EMIT_GROUP_LEDGER_OPERATION.to_owned(),
            env,
        }
    } else {
        StepKind::Shell {
            run: cargo_run_argv(),
            env,
        }
    };
    Step {
        parallel: false,
        name: EMIT_GROUP_LEDGER_STEP_NAME.to_owned(),
        id: None,
        role: None,
        condition: Some(crate::render::UPLOAD_STEP_CONDITION.to_owned()),
        kind,
    }
}

#[must_use]
pub(crate) fn assemble_group_ledgers_step() -> Step {
    Step {
        parallel: false,
        name: ASSEMBLE_GROUP_LEDGERS_STEP_NAME.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Internal {
            operation: ASSEMBLE_GROUP_LEDGERS_OPERATION.to_owned(),
            env: BTreeMap::new(),
        },
    }
}
