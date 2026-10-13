//! Qualified source bytes become retained, observed owned installation trees.
use crate::check_evidence::gate::tools::{QualifiedToolReceipt, receipt};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use velnor_actions_contract_config::config::{
    QualifiedCargoInstallation, QualifiedTool, QualifiedToolBackend, QualifiedToolOptions,
};
use velnor_actions_mise::check_tool_probes::{
    QualifiedExecutableObservation, QualifiedProbeHomes, verify_qualified_executable,
};
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, QualifiedCheck};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal;

mod archive;
mod layout;
mod tree;

pub(super) fn acquire(
    handle: &QualifiedCheck,
    check: &DiscoveredCheck,
    home: &Path,
    deadline: CheckDeadline,
) -> Result<Vec<QualifiedToolReceipt>, OrchestratorError> {
    let mut receipts = Vec::new();
    let mut expanded = archive::ArchiveBudget::new();
    for tool in &check.qualified_tools {
        let qualified = tool
            .platforms
            .iter()
            .find(|p| p.platform == check.check.runner.platform)
            .ok_or_else(|| internal("qualified_tool_platform"))?;
        let tool_home = handle
            .qualified_tool_home(&tool.id)
            .map_err(|e| internal(&e.to_string()))?;
        for directory in ["downloads", "verified", "unpacked", "cargo", "target"] {
            velnor_actions_orchestrator_core::exclusive_write::create_dir_no_symlink(
                home,
                &tool_home.join(directory),
            )?;
        }
        let primary = fetch_extract(
            handle,
            tool,
            false,
            &qualified.artifacts,
            &tool_home,
            deadline,
            &mut expanded,
        )?;
        let dependencies = fetch_extract(
            handle,
            tool,
            true,
            &qualified.dependency_artifacts,
            &tool_home,
            deadline,
            &mut expanded,
        )?;
        let prefix = tool_home.join("prefix");
        if source_mode(tool) {
            check_deadline(deadline)?;
            layout::prepare_cargo_source(
                tool,
                check.check.runner.platform,
                &tool_home,
                &primary,
                &dependencies,
                deadline,
            )?;
            check_deadline(deadline)?;
            run(handle.qualified_source_command(&tool.id), deadline)?;
        } else {
            check_deadline(deadline)?;
            layout::normalize_payload(
                tool,
                check.check.runner.platform,
                &primary,
                &prefix,
                deadline,
            )?;
            check_deadline(deadline)?;
        }
        tree::freeze_tree(&prefix, deadline)?;
        let actual_tree = tree::tree_sha256(&prefix, deadline)?;
        if actual_tree != qualified.install_tree_sha256 {
            return Err(internal("qualified_tool_install_tree_sha256"));
        }
        let proofs = observe_executables(
            tool,
            check,
            home,
            &prefix,
            compiler_prefix(tool, &check.qualified_tools, home),
            deadline,
        )?;
        receipts.push(receipt(
            tool,
            check.check.runner.platform,
            qualified.artifacts.clone(),
            qualified.dependency_artifacts.clone(),
            actual_tree,
            proofs,
        )?);
        run(handle.qualified_link_command(&tool.id), deadline)?;
    }
    Ok(receipts)
}

fn source_mode(tool: &QualifiedTool) -> bool {
    matches!(
        &tool.options,
        QualifiedToolOptions::Cargo {
            installation: QualifiedCargoInstallation::Source { .. },
            ..
        }
    )
}

fn fetch_extract(
    handle: &QualifiedCheck,
    tool: &QualifiedTool,
    dependency: bool,
    artifacts: &[velnor_actions_contract_config::config::QualifiedToolArtifact],
    tool_home: &Path,
    deadline: CheckDeadline,
    expanded: &mut archive::ArchiveBudget,
) -> Result<Vec<PathBuf>, OrchestratorError> {
    let mut roots = Vec::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        run(
            handle.qualified_fetch_command(&tool.id, dependency, index),
            deadline,
        )?;
        let downloaded = tool_home.join("downloads").join(&artifact.sha256);
        let bytes = read_with_deadline(&downloaded, 1024 * 1024 * 1024, deadline)
            .map_err(|error| staged_read_error(error, "qualified_tool_artifact_unreadable"))?;
        if sha256_with_deadline(&bytes, deadline)? != artifact.sha256 {
            return Err(internal("qualified_tool_artifact_sha256"));
        }
        let name = format!(
            "{}-{index}",
            if dependency { "dependency" } else { "primary" }
        );
        let verified = tool_home.join("verified").join(&name);
        velnor_actions_orchestrator_core::exclusive_write::write_exclusive_until(
            &verified,
            &bytes,
            "qualified_tool_archive",
            || check_deadline(deadline),
        )?;
        let root = tool_home.join("unpacked").join(name);
        archive::extract_archive(&verified, &root, &artifact.url, expanded, deadline)?;
        std::fs::remove_file(&downloaded)
            .map_err(|_| internal("qualified_tool_archive_cleanup"))?;
        std::fs::remove_file(&verified).map_err(|_| internal("qualified_tool_archive_cleanup"))?;
        roots.push(root);
    }
    Ok(roots)
}

fn compiler_prefix(tool: &QualifiedTool, tools: &[QualifiedTool], home: &Path) -> Option<PathBuf> {
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

fn observe_executables(
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
        super::link_program(path.as_os_str(), &home.join("bin").join(&executable.name))?;
        proofs.push(proof);
    }
    Ok(proofs)
}

fn run(
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

fn read_with_deadline(
    path: &Path,
    limit: u64,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, &'static str> {
    crate::retrieve_reports::staged_reads::read_staged_bytes_until(path, limit, || {
        deadline.remaining().map(|_| ()).map_err(|_| "deadline")
    })
}

fn sha256_with_deadline(
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

fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}

fn staged_read_error(problem: &'static str, context: &str) -> OrchestratorError {
    if problem == "deadline" {
        internal("check_timeout:deadline_exhausted")
    } else {
        internal(context)
    }
}

#[cfg(test)]
mod tests;
