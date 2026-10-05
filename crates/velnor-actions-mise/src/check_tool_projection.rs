//! Pure typed native tool configuration; acquisition authorization remains separate.
use super::{invalid, qualified};
use crate::MiseError;
use std::fmt::Write;
use velnor_actions_contract::config::{QualifiedTool, QualifiedToolOptions};
pub(super) fn config_for(tool: &QualifiedTool) -> Result<String, MiseError> {
    tool.validate("qualified_tool_projection", &tool.id)
        .map_err(|e| invalid("qualified_tool_projection", e.to_string()))?;
    let mut config = format!("[tools]\n{} = ", quote(&qualified::backend_key(tool)));
    match &tool.options {
        QualifiedToolOptions::Default => {
            config.push_str(&quote(&tool.version));
            config.push('\n');
        }
        QualifiedToolOptions::Rust {
            components,
            targets,
        } => {
            writeln!(
                config,
                "{{ version = {}, profile = \"minimal\", components = {}, targets = {} }}",
                quote(&tool.version),
                array(components),
                array(targets)
            )
            .map_err(|e| invalid("qualified_tool_projection", e.to_string()))?;
        }
        QualifiedToolOptions::Cargo {
            default_features,
            features,
            ..
        } => {
            writeln!(
                config,
                "{{ version = {}, default-features = {default_features}, features = {} }}",
                quote(&tool.version),
                quote(&features.join(","))
            )
            .map_err(|e| invalid("qualified_tool_projection", e.to_string()))?;
        }
    }
    Ok(config)
}
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('\"', "\\\""))
}
fn array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| quote(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}
