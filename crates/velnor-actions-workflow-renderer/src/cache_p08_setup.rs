//! Insert the typed V2 tools-cache prelude into rendered jobs.

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
        crate::tool_seed::reject_orphan_seed(job_id, job)?;
        return Ok(());
    }
    let expected_setup = cache_p08::setup_step(setup)?;
    let setup_index = if let Some(index) = setup_indices.first().copied() {
        if job.steps[index] != expected_setup {
            return Err(RenderError::InvalidWorkflow(format!(
                "setup_mise_malformed:{job_id}"
            )));
        }
        index
    } else {
        let at = insert_at(job, checkout_uses);
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

    let specs = cache_p08::infer_job_tools(job);
    let Some(setup_index) = insert_tool_seed(
        job,
        setup_index,
        setup,
        always,
        target,
        checkout_uses,
        &specs,
    )?
    else {
        return Ok(());
    };
    if specs.is_empty() {
        return Ok(());
    }
    insert_tools_cache(job_id, job, setup, target, &specs, setup_index)
}

fn insert_tool_seed(
    job: &mut Job,
    setup_index: usize,
    setup: &MiseSetup,
    always: bool,
    target: &str,
    checkout_uses: &str,
    specs: &[String],
) -> Result<Option<usize>, RenderError> {
    let seed_specs = if specs.is_empty() && always {
        vec!["mise@bootstrap".to_owned()]
    } else {
        specs.to_vec()
    };
    if seed_specs.is_empty() {
        return Ok(None);
    }
    let seed_key = cache_p08::MiseToolsCacheKey::derive(target, &setup.version, &seed_specs)?;
    crate::tool_seed::insert_before_setup(job, setup_index, checkout_uses, &seed_key).map(Some)
}

fn insert_tools_cache(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    target: &str,
    specs: &[String],
    setup_index: usize,
) -> Result<(), RenderError> {
    let rust_version = specs.iter().find_map(|spec| spec.strip_prefix("rust@"));
    let components = if let Some(version) = rust_version {
        job.steps
            .iter()
            .find(|step| step.name == "Prepare Rust components")
            .map(|step| cache_p08::rust_components(step, version, target))
            .transpose()?
            .unwrap_or_default()
    } else {
        if job
            .steps
            .iter()
            .any(|step| step.name == "Prepare Rust components")
        {
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
        tool_specs: specs,
        rustup_toolchain: rust_version,
        rustup_components: &components,
    })?;
    if !payload.runtime_identity_supported() {
        return Ok(());
    }
    let identity = payload.runtime_identity_step()?;
    let restore = payload.restore_step()?;
    job.steps
        .splice(setup_index..setup_index, [identity, restore]);
    Ok(())
}

fn reject_v2_steps(job_id: &str, job: &Job) -> Result<(), RenderError> {
    for name in [
        cache_p08::TOOLS_CACHE_IDENTITY_NAME,
        crate::cache_steps::TOOLS_RESTORE_NAME,
        crate::cache_steps::TOOLS_SAVE_NAME,
    ] {
        if job.steps.iter().any(|step| step.name == name) {
            return Err(RenderError::InvalidWorkflow(format!(
                "renderer_owned_tools_cache_step:{job_id}:{name}"
            )));
        }
    }
    Ok(())
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
        .position(|step| crate::tool_seed::is_configured_checkout(step, checkout_uses))
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
