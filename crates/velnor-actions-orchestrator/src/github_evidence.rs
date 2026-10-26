// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "github_evidence_collect.rs"]
mod collect;
#[path = "github_evidence_import.rs"]
mod import;

pub const GITHUB_EVIDENCE_FETCH_OP: &str = velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION;
pub const GITHUB_EVIDENCE_IMPORT_OP: &str =
    velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION;

pub fn run_github_evidence_fetch() -> Result<(), crate::OrchestratorError> {
    collect::run()
}

pub fn run_github_evidence_import() -> Result<(), crate::OrchestratorError> {
    import::run()
}
