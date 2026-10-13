//! Insert the typed V2 tools-cache prelude into rendered jobs.

use velnor_actions_contract::workflow::step_identity::is_configured_checkout;
use velnor_actions_contract::{Job, Step, StepKind};

use crate::{MiseSetup, RenderError, cache_p08, cache_p08::ToolsCacheInputs};

/// Add runtime identity, read-only restore, and cache-disabled Mise setup.
/// # Errors
pub(crate) fn ensure_tools_cache_v2(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    target: &str,
    checkout_uses: &str,
) -> Result<(), RenderError> {
    setup.validate()?;
    let setup_indices = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_setup_step(step))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if setup_indices.len() > 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_setup_mise:{job_id}"
        )));
    }
    let needed = always || job.steps.iter().any(step_uses_mise);
    if !needed && setup_indices.is_empty() {
        reject_v2_steps(job_id, job)?;
        return Ok(());
    }
    let expected_setup = cache_p08::setup_step(setup)?;
    let setup_index = if let Some(index) = setup_indices.first().copied() {
        if !cache_p08::same_step_semantics(&job.steps[index], &expected_setup) {
            return Err(RenderError::InvalidWorkflow(format!(
                "setup_mise_malformed:{job_id}"
            )));
        }
        index
    } else {
        let at = insert_at(job, checkout_uses).min(job.steps.len());
        job.steps.insert(at, expected_setup);
        at
    };
    if let Some(first_mise) = first_mise_index(job)
        && setup_index > first_mise
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_misordered:{job_id}"
        )));
    }
    reject_v2_steps(job_id, job)?;
    if !checkout_precedes_setup(job, setup_index, checkout_uses) {
        return Ok(());
    }
    insert_tools_prelude(job_id, job, setup, target, setup_index)
}

fn insert_tools_prelude(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    target: &str,
    setup_index: usize,
) -> Result<(), RenderError> {
    let specs = cache_p08::infer_job_tools(job);
    if specs.is_empty() {
        return Ok(());
    }
    let rust_version = specs.iter().find_map(|spec| spec.strip_prefix("rust@"));
    let components =
        if let Some(version) = rust_version {
            job.steps
                .iter()
                .find(|step| {
                    step.role == Some(velnor_actions_contract::StepRole::PrepareRustComponents)
                })
                .map(|step| cache_p08::rust_components(step, version, target))
                .transpose()?
                .unwrap_or_default()
        } else {
            if job.steps.iter().any(|step| {
                step.role == Some(velnor_actions_contract::StepRole::PrepareRustComponents)
            }) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "rust_components_without_toolchain:{job_id}"
                )));
            }
            Vec::new()
        };
    let payload = cache_p08::ToolsCachePayload::new(ToolsCacheInputs {
        runs_on: &job.runs_on,
        target,
        mise_setup: setup,
        tool_specs: &specs,
        rustup_toolchain: rust_version,
        rustup_components: &components,
    })?;
    if !payload.runtime_identity_supported() {
        return Ok(());
    }
    let prelude = payload.runtime_prelude_step()?;
    let restore = payload.restore_step()?;
    job.steps
        .splice(setup_index..setup_index, [prelude, restore]);
    Ok(())
}

fn checkout_precedes_setup(job: &Job, setup_index: usize, expected_uses: &str) -> bool {
    job.steps
        .iter()
        .position(|step| is_typed_checkout(step, expected_uses))
        .is_some_and(|checkout_index| checkout_index < setup_index)
}

fn is_typed_checkout(step: &Step, expected_uses: &str) -> bool {
    is_configured_checkout(step, expected_uses)
}

fn reject_v2_steps(job_id: &str, job: &Job) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| has_v2_cache_authority(step, job))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "renderer_owned_tools_cache_step:{job_id}"
        )));
    }
    Ok(())
}

fn has_v2_cache_authority(step: &Step, job: &Job) -> bool {
    if matches!(
        step.role,
        Some(
            velnor_actions_contract::StepRole::ToolSeed
                | velnor_actions_contract::StepRole::ToolsCacheIdentity
                | velnor_actions_contract::StepRole::ToolsCacheRestore
                | velnor_actions_contract::StepRole::ToolsCacheSave
        )
    ) {
        return true;
    }
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    if uses == velnor_actions_contract::workflow::step_identity::TOOL_SEED_USES {
        return true;
    }
    if cache_p08::runtime_identity_action_uses(&job.runs_on) == Some(uses.as_str())
        || cache_p08::runtime_prelude_action_uses(&job.runs_on) == Some(uses.as_str())
    {
        return true;
    }
    if uses == crate::cache_steps::TOOLS_RESTORE_USES {
        return true;
    }
    let tools_paths = crate::cache_steps::TOOLS_CACHE_PATHS
        .map(str::to_owned)
        .join("\n");
    uses == crate::cache_steps::TOOLS_RESTORE_ACTION_USES
        && with.get("path").map(String::as_str) == Some(tools_paths.as_str())
}

fn step_uses_mise(step: &Step) -> bool {
    let StepKind::Shell { run, .. } = &step.kind else {
        return false;
    };
    crate::cache_p08_detect::detector_words(run)
        .iter()
        .any(|word| word == "mise")
}

fn first_mise_index(job: &Job) -> Option<usize> {
    job.steps.iter().position(step_uses_mise)
}

fn is_setup_step(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, .. }
        if uses.starts_with(&format!("{}@", crate::setup::MISE_ACTION_NAME)))
}

fn insert_at(job: &Job, checkout_uses: &str) -> usize {
    job.steps
        .iter()
        .position(|step| is_typed_checkout(step, checkout_uses))
        .map_or(0, |index| index + 1)
}

/// Parse the exact pinned components step that populated this tool payload.
/// # Errors
pub(crate) fn rust_components(
    step: &Step,
    version: &str,
    target: &str,
) -> Result<Vec<String>, RenderError> {
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::BadCommand(
            "rust_components_not_shell".to_owned(),
        ));
    };
    if env
        .get("RUSTUP_TOOLCHAIN")
        .is_some_and(|value| value != version)
    {
        return Err(RenderError::BadCommand(
            "rust_components_toolchain_env_mismatch".to_owned(),
        ));
    }
    let words = crate::cache_p08_detect::detector_words(run);
    let Some(start) = words
        .windows(3)
        .position(|triple| triple == ["rustup", "component", "add"])
    else {
        return Err(RenderError::BadCommand(
            "rust_components_command_missing".to_owned(),
        ));
    };
    let payload = &words[start + 3..];
    if payload.len() < 3 || payload[0] != "--toolchain" {
        return Err(RenderError::BadCommand(
            "rust_components_toolchain_missing".to_owned(),
        ));
    }
    let expected = format!("{version}-{target}");
    if payload[1] != expected {
        return Err(RenderError::BadCommand(
            "rust_components_toolchain_mismatch".to_owned(),
        ));
    }
    let mut components = payload[2..].to_vec();
    components.sort();
    components.dedup();
    if components != ["clippy", "rustfmt"] {
        return Err(RenderError::BadCommand(
            "rust_components_set_mismatch".to_owned(),
        ));
    }
    Ok(components)
}

#[cfg(test)]
mod tests {
    use super::rust_components;
    use crate::steps::ambient_shell_step;
    use std::collections::BTreeMap;

    #[test]
    fn parses_the_fixed_rust_components_payload() {
        let step = ambient_shell_step(
            "Prepare Rust components",
            vec![
                "mise".to_owned(),
                "exec".to_owned(),
                "rust@1.98.1".to_owned(),
                "--".to_owned(),
                "rustup".to_owned(),
                "component".to_owned(),
                "add".to_owned(),
                "--toolchain".to_owned(),
                "1.98.1-x86_64-unknown-linux-gnu".to_owned(),
                "clippy".to_owned(),
                "rustfmt".to_owned(),
            ],
            BTreeMap::from([("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned())]),
        )
        .expect("fixed step");
        assert_eq!(
            rust_components(&step, "1.98.1", "x86_64-unknown-linux-gnu").expect("components"),
            ["clippy", "rustfmt"]
        );
    }
}
