//! Typed identity and path ownership for the external tools cache.
//!
//! The payload is built from resolved tool obligations and the expanded
//! job's runner selector. Runtime image and absolute-root identity are
//! appended by the later workflow identity step.

use velnor_actions_contract::{RunsOn, Step};

use crate::{MiseSetup, RenderError, cache_p08, steps};

#[path = "cache_p08_runtime_identity.rs"]
mod runtime_identity;
#[path = "cache_p08_runtime_prelude.rs"]
mod runtime_prelude;

pub(crate) fn runtime_identity_script_file(
    version: &str,
) -> Result<crate::tree::RenderedFile, RenderError> {
    runtime_identity::script_file(version)
}

pub(crate) fn runtime_identity_action_file(
    runs_on: &str,
    version: &str,
) -> Result<crate::tree::RenderedFile, RenderError> {
    runtime_identity::action_file(runs_on, version)
}

pub(crate) fn runtime_prelude_action_file(
    runs_on: &str,
    version: &str,
) -> Result<crate::tree::RenderedFile, RenderError> {
    runtime_prelude::action_file(runs_on, version)
}

pub(crate) fn validate_runtime_identity_action(
    step: &Step,
    uses: &str,
    runs_on: &str,
    with: &std::collections::BTreeMap<String, String>,
    env: &std::collections::BTreeMap<String, String>,
) -> Result<(), RenderError> {
    runtime_prelude::validate_action_call(step, uses, runs_on, with, env)
}

pub(crate) fn runtime_identity_action_uses(runs_on: &str) -> Option<&'static str> {
    runtime_identity::action_uses(runs_on)
}

pub(crate) fn runtime_prelude_action_uses(runs_on: &str) -> Option<&'static str> {
    runtime_prelude::action_uses(runs_on)
}

/// Inputs resolved from a job's typed preparation/catalog obligations.
#[derive(Debug, Clone, Copy)]
pub struct ToolsCacheInputs<'a> {
    /// Expanded job's exact hosted label or Scale Set token.
    pub runs_on: &'a str,
    /// Rust target selected for this job.
    pub target: &'a str,
    /// Exact validated Setup Mise action and binary pins used by this job.
    pub mise_setup: &'a MiseSetup,
    /// Exact catalog selectors installed by the job.
    pub tool_specs: &'a [String],
    /// Exact Rustup toolchain selected when Rust is present.
    pub rustup_toolchain: Option<&'a str>,
    /// Exact Rust components installed by the job.
    pub rustup_components: &'a [String],
}

/// Complete static portion of one exact tools-cache payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsCachePayload {
    static_digest: String,
    paths: Vec<String>,
    runs_on: String,
    target: String,
}

impl ToolsCachePayload {
    /// Build one canonical cache payload from typed pins and obligations.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for missing, loose, mismatched, unsupported,
    /// or malformed identity inputs.
    pub fn new(inputs: ToolsCacheInputs<'_>) -> Result<Self, RenderError> {
        validate_inputs(&inputs)?;
        let mut specs = inputs.tool_specs.to_vec();
        specs.sort();
        specs.dedup();
        let mut components = inputs.rustup_components.to_vec();
        components.sort();
        components.dedup();
        let paths: Vec<String> = steps::TOOLS_CACHE_PATHS
            .iter()
            .map(|path| (*path).to_owned())
            .collect();
        let preimage = identity_preimage(&inputs, &specs, &components, &paths);
        let digest = velnor_actions_contract::digest_b3(preimage.as_bytes());
        let static_digest = digest
            .get(3..)
            .ok_or_else(|| RenderError::BadCommand("bad_tools_cache_digest".to_owned()))?
            .to_owned();
        Ok(Self {
            static_digest,
            paths,
            runs_on: inputs.runs_on.to_owned(),
            target: inputs.target.to_owned(),
        })
    }

    /// Static BLAKE3 identity for tools, lane, platform, action pin, and paths.
    #[must_use]
    pub fn static_digest(&self) -> &str {
        &self.static_digest
    }

    /// Whether this lane has an exact hosted image identity for V2 restores.
    #[must_use]
    pub fn runtime_identity_supported(&self) -> bool {
        runtime_identity::is_supported_lane(&self.runs_on, &self.target)
    }

    /// Canonical exact paths archived by both restore and save.
    #[must_use]
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// Cache key expression bound to the renderer-owned runtime identity step.
    #[must_use]
    pub fn key_expression(&self) -> String {
        cache_p08::TOOLS_CACHE_KEY_EXPRESSION.to_owned()
    }

    /// Runtime identity and matching tool-seed step; unknown identities take a cold path.
    /// # Errors
    pub fn runtime_prelude_step(&self) -> Result<Step, RenderError> {
        runtime_prelude::step(self)
    }

    /// Concrete key builder for an exact lower-case SHA-256 runtime identity.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::BadCommand`] when the fingerprint is not
    /// exactly 64 lower-case hexadecimal characters.
    pub fn key_for_runtime_identity(&self, fingerprint: &str) -> Result<String, RenderError> {
        if !is_sha256(fingerprint) {
            return Err(RenderError::BadCommand(
                "bad_tools_cache_runtime_identity".to_owned(),
            ));
        }
        Ok(format!("mise-tools-v2-{fingerprint}"))
    }

    /// Read-only restore over the payload's key expression and exact paths.
    /// # Errors
    pub fn restore_step(&self) -> Result<Step, RenderError> {
        steps::tools_cache_step(
            true,
            &self.key_expression(),
            Some(cache_p08::TOOLS_CACHE_RESTORE_CONDITION.to_owned()),
        )
    }

    /// Save over the identical key expression and exact paths.
    /// # Errors
    pub fn save_step(&self) -> Result<Step, RenderError> {
        steps::tools_cache_step(
            false,
            &self.key_expression(),
            Some(super::save_policy::condition()),
        )
    }
}

fn validate_inputs(inputs: &ToolsCacheInputs<'_>) -> Result<(), RenderError> {
    RunsOn::parse(inputs.runs_on)
        .map_err(|err| RenderError::BadCommand(format!("bad_tools_cache_lane:{err}")))?;
    if !velnor_actions_contract::is_supported_target(inputs.target) {
        return Err(RenderError::BadCommand(format!(
            "bad_tools_cache_target:{}",
            inputs.target
        )));
    }
    inputs.mise_setup.validate()?;
    if inputs.tool_specs.is_empty() {
        return Err(RenderError::BadCommand(
            "empty_tools_cache_specs".to_owned(),
        ));
    }
    for spec in inputs.tool_specs {
        let Some((tool, version)) = spec.split_once('@') else {
            return Err(RenderError::BadCommand(format!(
                "bad_tools_cache_spec:{spec}"
            )));
        };
        if !cache_p08::is_tool_spec(spec) || !cache_p08::is_catalog_version(version) {
            return Err(RenderError::BadCommand(format!(
                "bad_tools_cache_spec:{spec}"
            )));
        }
        if inputs
            .tool_specs
            .iter()
            .filter(|candidate| candidate.starts_with(&format!("{tool}@")))
            .any(|candidate| candidate != spec)
        {
            return Err(RenderError::BadCommand(format!(
                "conflicting_tools_cache_spec:{tool}"
            )));
        }
    }
    validate_rust_inputs(inputs)
}

fn validate_rust_inputs(inputs: &ToolsCacheInputs<'_>) -> Result<(), RenderError> {
    let rust = inputs
        .tool_specs
        .iter()
        .find_map(|spec| spec.strip_prefix("rust@"));
    match (rust, inputs.rustup_toolchain) {
        (Some(version), Some(toolchain)) if version == toolchain => {}
        (None, None) if inputs.rustup_components.is_empty() => return Ok(()),
        _ => {
            return Err(RenderError::BadCommand(
                "tools_cache_rust_mismatch".to_owned(),
            ));
        }
    }
    if inputs
        .rustup_components
        .iter()
        .any(|component| !matches!(component.as_str(), "clippy" | "rustfmt"))
    {
        return Err(RenderError::BadCommand(
            "bad_tools_cache_rust_component".to_owned(),
        ));
    }
    Ok(())
}

fn identity_preimage(
    inputs: &ToolsCacheInputs<'_>,
    specs: &[String],
    components: &[String],
    paths: &[String],
) -> String {
    let mut fields = vec![
        "mise-tools-v2".to_owned(),
        inputs.runs_on.to_owned(),
        inputs.target.to_owned(),
        inputs.mise_setup.uses.clone(),
        inputs.mise_setup.version.clone(),
        inputs.mise_setup.sha256.clone(),
        inputs.rustup_toolchain.unwrap_or("").to_owned(),
    ];
    fields.extend(specs.iter().cloned());
    fields.push("components".to_owned());
    fields.extend(components.iter().cloned());
    fields.push("paths".to_owned());
    fields.extend(paths.iter().cloned());
    let mut preimage = String::new();
    for field in fields {
        preimage.push_str(&field.len().to_string());
        preimage.push(':');
        preimage.push_str(&field);
        preimage.push('\n');
    }
    preimage
}

fn is_sha(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_sha256(value: &str) -> bool {
    is_sha(value, 64)
}

#[cfg(test)]
#[path = "cache_p08_tool_payload_tests.rs"]
mod tests;
