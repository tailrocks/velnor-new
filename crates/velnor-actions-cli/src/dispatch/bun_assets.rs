// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;

use velnor_actions_orchestrator::{
    run_bun_assets_export, run_bun_assets_import, run_bun_assets_prepare,
    run_bun_assets_repository_check,
};

use super::operation::InternalOp;
use super::unit_exit;

pub(super) fn run(op: InternalOp) -> Option<ExitCode> {
    Some(match op {
        InternalOp::BunAssetsRepositoryCheck => unit_exit(run_bun_assets_repository_check()),
        InternalOp::BunAssetsExport => unit_exit(run_bun_assets_export()),
        InternalOp::BunAssetsPrepare => unit_exit(run_bun_assets_prepare()),
        InternalOp::BunAssetsImport => unit_exit(run_bun_assets_import()),
        _ => return None,
    })
}
