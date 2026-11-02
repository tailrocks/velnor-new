// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::ledger_cli::{
    LEDGER_CLI_BINARY_PATH, LEDGER_CLI_EMIT_CONDITION, LedgerCliSource,
};
use velnor_actions_contract::{Step, StepKind, WorkflowPolicy};

use crate::RenderError;

pub(crate) const EMIT_GROUP_LEDGER_OPERATION: &str = "emit-group-ledger-v1";

pub(crate) const ASSEMBLE_GROUP_LEDGERS_OPERATION: &str = "assemble-group-ledgers-v1";

pub(crate) const EMIT_GROUP_LEDGER_STEP_NAME: &str = "Emit group ledger";

pub(crate) const ASSEMBLE_GROUP_LEDGERS_STEP_NAME: &str = "Assemble group ledgers";

pub(crate) const JOB_STATUS_ENV: &str = "VELNOR_JOB_STATUS";

pub fn crate_group_ledger_steps(policy: WorkflowPolicy) -> Result<Vec<Step>, RenderError> {
    let mut env = BTreeMap::from([
        (
            crate::steps::INTERNAL_OP_ENV.to_owned(),
            EMIT_GROUP_LEDGER_OPERATION.to_owned(),
        ),
        (JOB_STATUS_ENV.to_owned(), "${{ job.status }}".to_owned()),
    ]);
    let (prefix, condition, kind) = match LedgerCliSource::for_policy(policy) {
        LedgerCliSource::Installed => {
            env.remove(crate::steps::INTERNAL_OP_ENV);
            (
                Vec::new(),
                crate::render::UPLOAD_STEP_CONDITION,
                StepKind::Internal {
                    operation: EMIT_GROUP_LEDGER_OPERATION.to_owned(),
                    env: crate::toolchain_env::with_credential_scrub(&env),
                },
            )
        }
        LedgerCliSource::SameRunBuild => (
            vec![
                crate::ledger_cli_steps::ledger_cli_download_step()?,
                crate::ledger_cli_steps::ledger_cli_verify_step()?,
            ],
            LEDGER_CLI_EMIT_CONDITION,
            StepKind::Shell {
                run: vec![LEDGER_CLI_BINARY_PATH.to_owned()],
                env: crate::toolchain_env::with_credential_scrub(&env),
            },
        ),
    };
    let emit = Step {
        parallel: false,
        name: EMIT_GROUP_LEDGER_STEP_NAME.to_owned(),
        id: None,
        role: None,
        condition: Some(condition.to_owned()),
        kind,
    };
    Ok(prefix.into_iter().chain([emit]).collect())
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
