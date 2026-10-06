//! Qualification identity: canonical fingerprints and tool projections.
use super::invalid;
use crate::{DiscoveredCheck, MiseError};
use std::fmt::Write;
use velnor_actions_contract::config::{
    QualifiedTool, QualifiedToolBackend, QualifiedToolOptions, validate_qualified_tools,
};
use velnor_actions_contract::{canonical_json_bytes, digest_b3};

impl DiscoveredCheck {
    /// Shared canonical fingerprint over full declarations and exact selected specs.
    /// # Errors
    /// Serialization failures reject the qualification identity.
    pub fn qualification_fingerprint(
        tools: &[QualifiedTool],
        specs: &[String],
    ) -> Result<String, MiseError> {
        fingerprint(tools, specs)
    }

    /// Refuse altered declarations, selectors or proposal qualification bindings.
    /// # Errors
    /// Every discovered qualification field must retain its exact canonical identity.
    pub fn verify_qualification_identity(&self) -> Result<(), MiseError> {
        let digest = Self::qualification_fingerprint(&self.qualified_tools, &self.tool_specs)?;
        let expected_flag = format!("qualified_tools:{digest}");
        let mut specs: Vec<&str> = self
            .proposal
            .identity
            .flags
            .iter()
            .filter_map(|flag| flag.strip_prefix("tool:"))
            .collect();
        specs.sort_unstable();
        let actual: Vec<&str> = self.tool_specs.iter().map(String::as_str).collect();
        if digest != self.qualification_digest
            || !self.proposal.identity.flags.contains(&expected_flag)
            || specs != actual
        {
            return Err(invalid("qualified_tool_identity", "qualification_changed"));
        }
        Ok(())
    }
    /// Source-free typed singleton config for one resolved installation record.
    /// # Errors
    /// Unknown IDs or invalid declaration slots fail before config construction.
    pub fn qualified_tool_config(&self, id: &str) -> Result<String, MiseError> {
        let tool = self
            .qualified_tools
            .iter()
            .find(|tool| tool.id == id)
            .ok_or_else(|| invalid("qualified_tool_projection", "unselected_tool_id"))?;
        config_for(tool)
    }
    /// Exact selected Rust channel, including explicit repository qualification.
    #[must_use]
    pub fn selected_rust_version(&self) -> Option<&str> {
        self.tool_specs
            .iter()
            .find_map(|spec| spec.strip_prefix("rust@"))
    }
}

/// Canonical identity is independent of the dependency installation schedule.
///
/// # Errors
///
/// Rejects unqualified tool declarations.
pub fn fingerprint(declarations: &[QualifiedTool], specs: &[String]) -> Result<String, MiseError> {
    let mut declarations = declarations.to_vec();
    declarations.sort_by(|left, right| left.id.cmp(&right.id));
    validate_qualified_tools(&declarations, "qualification_fingerprint")
        .map_err(|e| invalid("qualified_tools", e.to_string()))?;
    let mut specs = specs.to_vec();
    specs.sort();
    let mut declared_specs: Vec<String> = declarations.iter().map(selector).collect();
    declared_specs.sort();
    if specs != declared_specs {
        return Err(invalid(
            "qualified_tools",
            "selectors_do_not_match_qualified_closure",
        ));
    }
    let bytes = canonical_json_bytes(&(&declarations, &specs))
        .map_err(|e| invalid("qualified_tools", e.to_string()))?;
    Ok(digest_b3(&bytes))
}

/// Render the `[tools]` TOML stanza pinning one qualified tool.
///
/// # Errors
///
/// Returns [`MiseError::InvalidStepInput`] when the tool record or version
/// fails validation.
pub fn config_for(tool: &QualifiedTool) -> Result<String, MiseError> {
    tool.validate("qualified_tool_projection", &tool.id)
        .map_err(|e| invalid("qualified_tool_projection", e.to_string()))?;
    let mut config = format!("[tools]\n{} = ", quote(&backend_key(tool)));
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

/// Exact backend selector; options and qualification bind separately in the fingerprint.
#[must_use]
pub fn selector(tool: &QualifiedTool) -> String {
    format!("{}@{}", backend_key(tool), tool.version)
}
/// Backend-qualified install key (`aqua:`/`cargo:` prefixed, core bare).
#[must_use]
pub fn backend_key(tool: &QualifiedTool) -> String {
    match &tool.backend {
        QualifiedToolBackend::Core { tool } => tool.clone(),
        QualifiedToolBackend::Aqua { package } => format!("aqua:{package}"),
        QualifiedToolBackend::Cargo { crate_name } => format!("cargo:{crate_name}"),
    }
}
