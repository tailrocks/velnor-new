// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_contract::HelperError;
use velnor_actions_contract::workflow::group_ledger::{
    ASSEMBLE_GROUP_LEDGERS_OP, EMIT_GROUP_LEDGER_OP, assemble_group_ledgers, emit_group_ledger,
};
#[cfg(not(feature = "generator"))]
use velnor_actions_contract::workflow::ledger_cli::LEDGER_CLI_REJECTION_MARKER;

const OP_ENV: &str = "VELNOR_INTERNAL_OP";
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LedgerOp {
    Emit,
    Assemble,
}

fn op_from_env() -> Option<LedgerOp> {
    match env::var(OP_ENV).as_deref() {
        Ok(tag) if tag == EMIT_GROUP_LEDGER_OP => Some(LedgerOp::Emit),
        Ok(tag) if tag == ASSEMBLE_GROUP_LEDGERS_OP => Some(LedgerOp::Assemble),
        _ => None,
    }
}

fn runner_temp_is_absolute() -> bool {
    env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .is_some_and(|path| path.is_absolute())
}

pub(crate) fn try_entry() -> Option<ExitCode> {
    if env::args_os().len() > 1 {
        return None;
    }
    let op = op_from_env()?;
    if !runner_temp_is_absolute() {
        return None;
    }
    Some(report(match op {
        LedgerOp::Emit => emit_group_ledger(),
        LedgerOp::Assemble => assemble_group_ledgers(),
    }))
}

fn report(outcome: Result<PathBuf, HelperError>) -> ExitCode {
    match outcome {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("velnor-actions: internal request failed: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(feature = "generator"))]
pub(crate) fn reject() -> ExitCode {
    eprintln!(
        "velnor-actions: this {LEDGER_CLI_REJECTION_MARKER} {EMIT_GROUP_LEDGER_OP} and {ASSEMBLE_GROUP_LEDGERS_OP} through {OP_ENV}"
    );
    ExitCode::from(2)
}
