use super::{OrchestratorError, acquisition, checkpoint, internal, io, request};
use std::path::{Path, PathBuf};
use velnor_actions_contract::config::{
    CheckPlatform, QualifiedTool, QualifiedToolArtifact, QualifiedToolExecutable,
    QualifiedToolPlatform,
};
use velnor_actions_mise::CheckDeadline;

#[derive(Debug, serde::Serialize)]
pub(super) struct ArchiveObservation {
    tool_id: String,
    url: String,
    declared_sha256: String,
    observed_sha256: String,
    bytes: usize,
}

pub(super) fn stage_archives(
    request: &request::Request,
    work: &Path,
    deadline: CheckDeadline,
) -> Result<Vec<ArchiveObservation>, OrchestratorError> {
    let mut observations = Vec::new();
    let mut remaining = 4_u64 * 1024 * 1024 * 1024;
    for tool in &request.tools {
        let home = work.join("tools").join(&tool.recipe.id);
        io::directory(&home)?;
        io::directory(&home.join("verified"))?;
        io::directory(&home.join("unpacked"))?;
        for (index, archive) in tool.archives.iter().enumerate() {
            checkpoint(deadline)?;
            let bytes = io::read(&archive.path, remaining.min(1024 * 1024 * 1024), deadline)?;
            remaining = remaining.saturating_sub(bytes.len() as u64);
            let observed = acquisition::sha256_with_deadline(&bytes, deadline)?;
            if observed != archive.sha256 {
                return Err(internal("qualification_archive_sha256_mismatch"));
            }
            observations.push(ArchiveObservation {
                tool_id: tool.recipe.id.clone(),
                url: archive.url.clone(),
                declared_sha256: archive.sha256.clone(),
                observed_sha256: observed,
                bytes: bytes.len(),
            });
            io::write(
                &home.join("verified").join(index.to_string()),
                &bytes,
                deadline,
            )?;
        }
    }
    Ok(observations)
}

pub(super) fn install(
    tool: &request::Tool,
    work: &Path,
    platform: CheckPlatform,
    budget: &mut acquisition::archive::ArchiveBudget,
    deadline: CheckDeadline,
) -> Result<QualifiedTool, OrchestratorError> {
    let home = work.join("tools").join(&tool.recipe.id);
    let mut roots = Vec::new();
    for (index, archive) in tool.archives.iter().enumerate() {
        let verified = home.join("verified").join(index.to_string());
        let staged = io::read(&verified, 1024 * 1024 * 1024, deadline)?;
        if acquisition::sha256_with_deadline(&staged, deadline)? != archive.sha256 {
            return Err(internal("qualification_staged_archive_sha256_mismatch"));
        }
        drop(staged);
        let root = home.join("unpacked").join(index.to_string());
        acquisition::archive::extract_archive(&verified, &root, &archive.url, budget, deadline)?;
        roots.push(root);
    }
    let prefix = home.join("prefix");
    acquisition::layout::normalize_recipe_payload(
        &tool.recipe,
        platform,
        &roots,
        &prefix,
        deadline,
    )?;
    acquisition::tree::freeze_tree(&prefix, deadline)?;
    let tree = acquisition::tree::tree_sha256(&prefix, deadline)?;
    let mut executables = Vec::new();
    for executable in &tool.recipe.platforms[0].executables {
        let sha256 = executable_hash(&prefix, &executable.path, deadline)?;
        executables.push(QualifiedToolExecutable {
            name: executable.name.clone(),
            path: executable.path.clone(),
            sha256,
            probe: executable.probe.clone(),
        });
    }
    let completed = QualifiedTool {
        id: tool.recipe.id.clone(),
        backend: tool.recipe.backend.clone(),
        version: tool.recipe.version.clone(),
        options: tool.recipe.options.clone(),
        depends_on: tool.recipe.depends_on.clone(),
        platforms: vec![QualifiedToolPlatform {
            platform,
            artifacts: tool
                .archives
                .iter()
                .map(|a| QualifiedToolArtifact {
                    url: a.url.clone(),
                    sha256: a.sha256.clone(),
                })
                .collect(),
            dependency_artifacts: Vec::new(),
            install_tree_sha256: tree,
            executables,
        }],
    };
    completed
        .validate("qualification_candidate", &completed.id)
        .map_err(crate::internal::internal_contract)?;
    Ok(completed)
}

pub(super) fn executable_hash(
    prefix: &Path,
    relative: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    crate::check_evidence::reject_link_components(prefix, relative)?;
    let path = prefix.join(relative);
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| internal("qualification_executable_unreadable"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(internal("qualification_executable_mode"));
        }
    }
    acquisition::executable_sha256(prefix, relative, deadline)
}

pub(super) fn prefix(work: &Path, tool: &QualifiedTool) -> PathBuf {
    work.join("tools").join(&tool.id).join("prefix")
}

pub(super) fn verify_unchanged(
    tools: &[QualifiedTool],
    work: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let platform = tools
        .first()
        .and_then(|tool| tool.platforms.first())
        .ok_or_else(|| internal("qualification_empty_installation"))?
        .platform;
    acquisition::validate_installations(tools, platform, work, deadline)
}
