// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::process::ExitCode;

use super::fail_internal;
use super::operation::InternalOp;
use super::{InternalRequest, RUNNER_TEMP_ENV};

/// Gate the tokenless group-ledger emit on the runner temp dir.
///
/// Like the other `RUNNER_TEMP`-gated internal ops, emit needs no request
/// file: the checked-in manifest, the downloaded plan, and the staged
/// reports are all addressed from `RUNNER_TEMP/velnor/<run_key>`.
pub(super) fn gate(op: InternalOp) -> Option<InternalRequest> {
    if op != InternalOp::EmitGroupLedger {
        return None;
    }
    let runner_temp = std::env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    if !runner_temp.is_absolute() {
        return None;
    }
    Some(InternalRequest {
        op,
        path: runner_temp.join("velnor"),
    })
}

pub(super) fn run() -> ExitCode {
    match velnor_actions_orchestrator::emit_group_ledger() {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => fail_internal(&error.to_string()),
    }
}

// gc1-t55 pins the emit op wiring: the op string matches the orchestrator
// operation, and gating never claims a foreign op.
#[cfg(test)]
mod gc1_t55 {
    use super::super::operation::InternalOp;
    use super::gate;

    #[test]
    fn op_string_matches_orchestrator() {
        assert_eq!(
            velnor_actions_orchestrator::EMIT_GROUP_LEDGER_OP,
            "emit-group-ledger-v1"
        );
    }

    #[test]
    fn gate_rejects_foreign_ops() {
        assert!(gate(InternalOp::Fetch).is_none());
        assert!(gate(InternalOp::AssembleGroupLedgers).is_none());
    }
}
