// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition;
use crate::internal::internal;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;
use velnor_actions_contract::config::{CheckPlatform, QualifiedTool, validate_qualified_tools};
use velnor_actions_mise::CheckDeadline;
use velnor_actions_mise::check_tool_probes::QualifiedExecutableProof;

#[path = "qualification_candidate_install.rs"]
mod install;
#[path = "qualification_candidate_io.rs"]
mod io;
#[path = "qualification_candidate_probe.rs"]
mod probe;
#[path = "qualification_candidate_request.rs"]
mod request;

#[derive(Debug, Serialize)]
struct Candidate {
    schema_version: u32,
    evidence_kind: &'static str,
    platform: CheckPlatform,
    request_sha256: String,
    execution_order: Vec<String>,
    qualified_tools: Vec<QualifiedTool>,
    probes: Vec<QualifiedExecutableProof>,
    archives: Vec<install::ArchiveObservation>,
}

pub fn qualify_tools(
    request_path: &Path,
    work_dir: &Path,
    output: &Path,
) -> Result<(), OrchestratorError> {
    let (work, output) = io::new_paths(work_dir, output)?;
    let initial =
        CheckDeadline::after(Duration::from_secs(60)).map_err(|e| internal(&e.to_string()))?;
    let bytes = io::read(request_path, request::MAX_REQUEST_BYTES, initial)?;
    let request = request::parse(&bytes)?;
    let deadline = CheckDeadline::after(Duration::from_secs(u64::from(request.timeout_seconds)))
        .map_err(|e| internal(&e.to_string()))?;
    let request_sha256 = acquisition::sha256_with_deadline(&bytes, deadline)?;
    io::create_home(&work)?;
    let archives = install::stage_archives(&request, &work, deadline)?;
    let mut candidate = Candidate {
        schema_version: 1,
        evidence_kind: "qualified_tool_authoring_candidate",
        platform: request.platform,
        request_sha256,
        execution_order: request
            .tools
            .iter()
            .map(|tool| tool.recipe.id.clone())
            .collect(),
        qualified_tools: Vec::new(),
        probes: Vec::new(),
        archives,
    };
    let mut budget = acquisition::archive::ArchiveBudget::new();
    for tool in &request.tools {
        checkpoint(deadline)?;
        let completed = install::install(tool, &work, request.platform, &mut budget, deadline)?;
        let proofs = probe::observe(
            &completed,
            &candidate.qualified_tools,
            &work,
            request.platform,
            deadline,
        )?;
        candidate.probes.extend(proofs);
        candidate.qualified_tools.push(completed);
    }
    install::verify_unchanged(&candidate.qualified_tools, &work, deadline)?;
    candidate.qualified_tools.sort_by(|a, b| a.id.cmp(&b.id));
    validate_qualified_tools(&candidate.qualified_tools, "qualification_candidate")
        .map_err(crate::internal::internal_contract)?;
    let bytes = velnor_actions_contract::canonical_json_bytes(&candidate)
        .map_err(crate::internal::internal_contract)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(internal("qualification_candidate_output_limit"));
    }
    io::write(&output, &bytes, deadline)
}

fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|e| internal(&e.to_string()))
}
