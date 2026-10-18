//! Complete archive/tree/executable evidence admission for qualified tools.
use super::{ContractError, QualifiedTool, QualifiedToolRecipe};

pub(super) fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

pub(super) fn validate_tool(
    tool: &QualifiedTool,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    super::recipe_validation::validate_recipe(&QualifiedToolRecipe::from(tool), file, key)?;
    let bad = || ContractError::config(file, key, "invalid_qualified_tool_measurement");
    for platform in &tool.platforms {
        if !sha256(&platform.install_tree_sha256)
            || platform
                .artifacts
                .iter()
                .any(|artifact| !sha256(&artifact.sha256))
            || platform
                .dependency_artifacts
                .iter()
                .any(|artifact| !sha256(&artifact.sha256))
            || platform.executables.iter().any(|exe| !sha256(&exe.sha256))
        {
            return Err(bad());
        }
    }
    Ok(())
}

pub(super) fn validate_registry(tools: &[QualifiedTool], file: &str) -> Result<(), ContractError> {
    let recipes: Vec<_> = tools.iter().map(QualifiedToolRecipe::from).collect();
    super::recipe_validation::validate_registry(&recipes, file)?;
    for (index, tool) in tools.iter().enumerate() {
        validate_tool(tool, file, &format!("qualified_tools[{index}]"))?;
    }
    Ok(())
}
