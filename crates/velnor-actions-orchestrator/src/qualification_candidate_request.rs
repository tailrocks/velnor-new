// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OrchestratorError, internal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use velnor_actions_contract::config::{
    CheckPlatform, QualifiedCargoInstallation, QualifiedToolBackend, QualifiedToolOptions,
    QualifiedToolRecipe, validate_qualified_tool_recipes,
};

pub(super) const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub schema_version: u32,
    pub platform: CheckPlatform,
    pub timeout_seconds: u32,
    pub tools: Vec<Tool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tool {
    pub recipe: QualifiedToolRecipe,
    pub archives: Vec<LocalArchive>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocalArchive {
    pub path: PathBuf,
    pub url: String,
    pub sha256: String,
}

pub(super) fn parse(bytes: &[u8]) -> Result<Request, OrchestratorError> {
    let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(
        bytes,
        usize::try_from(MAX_REQUEST_BYTES).map_err(|_| internal("qualification_request_limit"))?,
    )
    .map_err(crate::internal::internal_contract)?;
    let request: Request =
        serde_json::from_value(value).map_err(|_| internal("qualification_request_shape"))?;
    validate(&request)?;
    Ok(request)
}

fn validate(request: &Request) -> Result<(), OrchestratorError> {
    if request.schema_version != 1
        || !(1..=3600).contains(&request.timeout_seconds)
        || request.tools.is_empty()
        || request.tools.len() > 32
    {
        return Err(internal("qualification_request_bounds"));
    }
    if request.platform.os() != std::env::consts::OS
        || request.platform.arch() != std::env::consts::ARCH
    {
        return Err(internal("qualification_native_platform_required"));
    }
    let mut recipes: Vec<_> = request.tools.iter().map(|t| t.recipe.clone()).collect();
    recipes.sort_by(|a, b| a.id.cmp(&b.id));
    validate_qualified_tool_recipes(&recipes, "qualification_request")
        .map_err(crate::internal::internal_contract)?;
    let mut admitted = BTreeSet::new();
    for tool in &request.tools {
        validate_tool(tool, request.platform)?;
        if !tool
            .recipe
            .depends_on
            .iter()
            .all(|id| admitted.contains(id))
            || !admitted.insert(tool.recipe.id.clone())
        {
            return Err(internal("qualification_dependency_order"));
        }
    }
    Ok(())
}

fn validate_tool(tool: &Tool, platform: CheckPlatform) -> Result<(), OrchestratorError> {
    let supported = match &tool.recipe.backend {
        QualifiedToolBackend::Core { tool } => matches!(tool.as_str(), "rust" | "zig" | "python"),
        QualifiedToolBackend::Aqua { .. } => true,
        QualifiedToolBackend::Cargo { .. } => false,
    };
    if !supported
        || tool.recipe.platforms.len() != 1
        || tool.recipe.platforms[0].platform != platform
        || tool.archives.is_empty()
        || tool.archives.len() > 32
        || tool.recipe.platforms[0].executables.len() > 32
        || !tool.recipe.platforms[0].dependency_artifacts.is_empty()
        || matches!(
            tool.recipe.options,
            QualifiedToolOptions::Cargo {
                installation: QualifiedCargoInstallation::Source { .. },
                ..
            }
        )
    {
        return Err(internal("qualification_offline_prebuilt_recipe_required"));
    }
    let declared = &tool.recipe.platforms[0].artifacts;
    if declared.len() != tool.archives.len()
        || declared.iter().zip(&tool.archives).any(|(url, local)| {
            url.url != local.url
                || !velnor_actions_contract::ids::is_lower_hex_len(&local.sha256, 64)
                || local.sha256.bytes().all(|byte| byte == b'0')
        })
    {
        return Err(internal("qualification_independent_archive_identity"));
    }
    for archive in &tool.archives {
        super::io::absolute_path(&archive.path)?;
    }
    Ok(())
}
