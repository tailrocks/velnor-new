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

#[derive(Debug, Clone, Copy)]
pub struct ToolsCacheInputs<'a> {
    pub runs_on: &'a str,
    pub target: &'a str,
    pub mise_setup: &'a MiseSetup,
    pub tool_specs: &'a [String],
    pub rustup_toolchain: Option<&'a str>,
    pub rustup_components: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsCachePayload {
    static_digest: String,
    project_tools: Vec<String>,
    mise_sha256: String,
    paths: Vec<String>,
    runs_on: String,
    target: String,
}

impl ToolsCachePayload {
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
        let project_tools = specs
            .iter()
            .flat_map(|spec| {
                velnor_actions_contract::config::project_tool_refs::project_tool_ref_names(spec)
            })
            .map(str::to_owned)
            .collect();
        Ok(Self {
            static_digest,
            project_tools,
            mise_sha256: inputs.mise_setup.sha256.clone(),
            paths,
            runs_on: inputs.runs_on.to_owned(),
            target: inputs.target.to_owned(),
        })
    }

    #[must_use]
    pub fn static_digest(&self) -> &str {
        &self.static_digest
    }

    #[must_use]
    pub fn identity_input(&self) -> String {
        velnor_actions_contract::config::project_tool_refs::tools_identity_input(
            &self.static_digest,
            self.project_tools.iter().map(String::as_str),
        )
    }

    #[must_use]
    pub fn runtime_identity_supported(&self) -> bool {
        runtime_identity::is_supported_lane(&self.runs_on, &self.target)
    }

    #[must_use]
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    #[must_use]
    pub fn key_expression(&self) -> String {
        cache_p08::TOOLS_CACHE_KEY_EXPRESSION.to_owned()
    }

    pub fn runtime_prelude_step(&self) -> Result<Step, RenderError> {
        runtime_prelude::step(self)
    }

    pub fn key_for_runtime_identity(&self, fingerprint: &str) -> Result<String, RenderError> {
        if !is_sha256(fingerprint) {
            return Err(RenderError::BadCommand(
                "bad_tools_cache_runtime_identity".to_owned(),
            ));
        }
        Ok(format!("mise-tools-v2-{fingerprint}"))
    }

    pub fn restore_step(&self) -> Result<Step, RenderError> {
        steps::tools_cache_restore_step(
            &self.key_expression(),
            &self.mise_sha256,
            Some(cache_p08::TOOLS_CACHE_RESTORE_CONDITION.to_owned()),
        )
    }

    pub fn save_step(&self) -> Result<Step, RenderError> {
        steps::tools_cache_save_step(
            &self.key_expression(),
            Some(super::save_policy::condition(
                velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
            )),
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
                "conflicting_tools_cache_spec:{tool}:{}",
                inputs.tool_specs.join(",")
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
