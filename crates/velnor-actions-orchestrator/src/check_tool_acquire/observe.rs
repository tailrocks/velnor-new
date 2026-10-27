// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use velnor_actions_contract::config::{QualifiedTool, QualifiedToolBackend};
use velnor_actions_mise::check_tool_probes::{
    QualifiedExecutableObservation, QualifiedProbeHomes, verify_qualified_executable,
};
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck};

use super::super::link_program;
use super::tree;
use crate::OrchestratorError;
use crate::internal::internal;

pub(super) fn compiler_prefix(
    tool: &QualifiedTool,
    tools: &[QualifiedTool],
    home: &Path,
) -> Option<PathBuf> {
    let compiler = match &tool.backend {
        QualifiedToolBackend::Core { tool: name } if name == "rust" => Some(tool),
        _ if tool.requires_compiler() => tools.iter().find(|candidate| {
            tool.depends_on.contains(&candidate.id)
                && matches!(&candidate.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
        }),
        _ => None,
    };
    compiler.map(|tool| home.join("tools").join(&tool.id).join("prefix"))
}

pub(super) fn observe_executables(
    tool: &QualifiedTool,
    check: &DiscoveredCheck,
    home: &Path,
    prefix: &Path,
    compiler_toolchain: Option<PathBuf>,
    deadline: CheckDeadline,
) -> Result<Vec<velnor_actions_mise::check_tool_probes::QualifiedExecutableProof>, OrchestratorError>
{
    let qualified = tool
        .platforms
        .iter()
        .find(|p| p.platform == check.check.runner.platform)
        .ok_or_else(|| internal("qualified_tool_platform"))?;
    let homes = QualifiedProbeHomes {
        home: home.into(),
        bin_dir: home.join("bin"),
        cargo_home: home.join("cargo"),
        rust_home: home.join(velnor_actions_mise::checks::RUSTUP_HOME_SUFFIX),
        compiler_toolchain,
    };
    let mut proofs = Vec::new();
    for executable in &qualified.executables {
        crate::check_evidence::reject_link_components(prefix, &executable.path)?;
        let path = prefix.join(&executable.path);
        let bytes = read_with_deadline(&path, 256 * 1024 * 1024, deadline)
            .map_err(|error| staged_read_error(error, "qualified_tool_executable_unreadable"))?;
        let sha256 = sha256_with_deadline(&bytes, deadline)?;
        if sha256 != executable.sha256 {
            return Err(internal("qualified_tool_executable_sha256"));
        }
        let observed = QualifiedExecutableObservation {
            name: executable.name.clone(),
            path: path.clone(),
            sha256,
        };
        let proof = verify_qualified_executable(
            tool,
            check.check.runner.platform,
            executable,
            &observed,
            &homes,
            deadline,
        )
        .map_err(|e| internal(&e.to_string()))?;
        link_program(path.as_os_str(), &home.join("bin").join(&executable.name))?;
        proofs.push(proof);
    }
    Ok(proofs)
}

pub(crate) fn validate_installations(
    tools: &[QualifiedTool],
    platform: velnor_actions_contract::config::CheckPlatform,
    home: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for tool in tools {
        let qualified = tool
            .platforms
            .iter()
            .find(|p| p.platform == platform)
            .ok_or_else(|| internal("qualified_tool_platform"))?;
        let prefix = home.join("tools").join(&tool.id).join("prefix");
        tree::freeze_tree(&prefix, deadline)?;
        if tree::tree_sha256(&prefix, deadline)? != qualified.install_tree_sha256 {
            return Err(internal("qualified_tool_install_tree_sha256"));
        }
        for executable in &qualified.executables {
            if executable_sha256(&prefix, &executable.path, deadline)? != executable.sha256 {
                return Err(internal("qualified_tool_executable_sha256"));
            }
        }
    }
    Ok(())
}

pub(crate) fn executable_sha256(
    prefix: &Path,
    relative: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    crate::check_evidence::reject_link_components(prefix, relative)?;
    let path = prefix.join(relative);
    let bytes = read_with_deadline(&path, 256 * 1024 * 1024, deadline)
        .map_err(|error| staged_read_error(error, "qualified_tool_executable_unreadable"))?;
    sha256_with_deadline(&bytes, deadline)
}

pub(super) fn run(
    command: Result<velnor_actions_mise::IsolatedCommand, velnor_actions_mise::MiseError>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let output = command
        .map_err(|e| internal(&e.to_string()))?
        .run_until(8 * 1024 * 1024, deadline)
        .map_err(|e| internal(&e.to_string()))?;
    output
        .require_success("qualified-tool-acquisition")
        .map_err(|e| internal(&e.to_string()))?;
    Ok(())
}

pub(crate) fn read_with_deadline(
    path: &Path,
    limit: u64,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, &'static str> {
    crate::retrieve_reports::staged_reads::read_staged_bytes_until(path, limit, || {
        deadline.remaining().map(|_| ()).map_err(|_| "deadline")
    })
}

pub(crate) fn sha256_with_deadline(
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let mut hash = Sha256::new();
    for chunk in bytes.chunks(64 * 1024) {
        check_deadline(deadline)?;
        hash.update(chunk);
    }
    check_deadline(deadline)?;
    let digest = hash.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").map_err(|_| internal("qualified_tool_sha256"))?;
    }
    Ok(hex)
}

pub(crate) fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}

pub(super) fn staged_read_error(problem: &'static str, context: &str) -> OrchestratorError {
    if problem == "deadline" {
        internal("check_timeout:deadline_exhausted")
    } else {
        internal(context)
    }
}
