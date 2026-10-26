// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OrchestratorError, install, internal};
use std::path::Path;
use velnor_actions_contract::config::{CheckPlatform, QualifiedTool, QualifiedToolBackend};
use velnor_actions_mise::CheckDeadline;
use velnor_actions_mise::check_tool_probes::{
    QualifiedExecutableObservation, QualifiedExecutableProof, QualifiedProbeHomes,
    validate_executable_proofs, verify_qualified_executable,
};

pub(super) fn observe(
    tool: &QualifiedTool,
    prior: &[QualifiedTool],
    work: &Path,
    platform: CheckPlatform,
    deadline: CheckDeadline,
) -> Result<Vec<QualifiedExecutableProof>, OrchestratorError> {
    let prefix = install::prefix(work, tool);
    let compiler = if matches!(&tool.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
    {
        Some(prefix.clone())
    } else {
        prior.iter().find(|candidate| tool.depends_on.contains(&candidate.id)
            && matches!(&candidate.backend, QualifiedToolBackend::Core { tool } if tool == "rust"))
            .map(|candidate| install::prefix(work, candidate))
    };
    let homes = QualifiedProbeHomes {
        home: work.to_owned(),
        bin_dir: work.join("bin"),
        cargo_home: work.join("cargo"),
        rust_home: work.join("rustup"),
        compiler_toolchain: compiler,
    };
    let mut proofs = Vec::new();
    for executable in &tool.platforms[0].executables {
        let path = prefix.join(&executable.path);
        let observed = QualifiedExecutableObservation {
            name: executable.name.clone(),
            path: path.clone(),
            sha256: install::executable_hash(&prefix, &executable.path, deadline)?,
        };
        proofs.push(
            verify_qualified_executable(tool, platform, executable, &observed, &homes, deadline)
                .map_err(|e| internal(&e.to_string()))?,
        );
    }
    validate_executable_proofs(tool, platform, &proofs).map_err(|e| internal(&e.to_string()))?;
    for executable in &tool.platforms[0].executables {
        let source = prefix.join(&executable.path);
        let destination = work.join("bin").join(&executable.name);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source, &destination)
            .map_err(|_| internal("qualification_executable_projection"))?;
        #[cfg(not(unix))]
        std::fs::hard_link(&source, &destination)
            .map_err(|_| internal("qualification_executable_projection"))?;
    }
    Ok(proofs)
}
