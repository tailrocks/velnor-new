//! Read-only tool seed at the fixed container path `/opt/velnor/seed`.
//!
//! A previous step cannot choose this path. A missing seed or a different
//! key stays cold. The job copies into its private homes. It does not
//! delete or write the seed.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::workflow::step_identity::{
    TOOL_SEED_USES, is_configured_checkout, is_tool_seed_step,
};
use velnor_actions_contract_workflow::{Job, Step, StepKind, StepRole};

use crate::cache_p08::MiseToolsCacheKey;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::yaml::Yaml;

use crate::tool_seed_admission::trusted_seed_guard;
pub(crate) use crate::tool_seed_admission::{SEED_ROOT, require_seed_root};

/// Display name of the copy step ahead of `Setup Mise`.
pub(crate) const TOOL_SEED_NAME: &str = "Restore Velnor tool seed";
/// Repository path of that composite.
const TOOL_SEED_ACTION_PATH: &str = ".github/actions/velnor-tool-seed/action.yml";

/// Copy script. The composite sets `SEED_KEY` from `inputs.cache_key`.
///
/// The script does not embed a job-specific key, so one action file serves
/// every job. An empty key does not match. Rustup lands at
/// `$RUNNER_TEMP/velnor/rustup`, the same path as `MISE_RUSTUP_HOME`.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for a bad root.
pub(crate) fn tool_seed_action_script(seed_root: &str) -> Result<String, RenderError> {
    require_seed_root(seed_root)?;
    let guard = trusted_seed_guard(seed_root)?;
    Ok(copy_script(seed_root, "\"$SEED_KEY\"", &guard))
}

fn copy_script(seed_root: &str, key_shell: &str, guard: &str) -> String {
    format!(
        r#"set -euo pipefail; {guard}; seed="{seed_root}"; key={key_shell}; if [ ! -e "$seed" ]; then echo "tool seed absent"; exit 0; fi; if ! trusted_seed_is_trusted "$seed"; then echo "untrusted tool seed; continuing cold"; exit 0; fi; if [ -z "$key" ] || [ ! -f "$seed/mise/KEY" ]; then echo "tool seed key mismatch"; exit 0; fi; if ! trusted_seed_file_matches "$seed/mise/KEY" "$key"; then echo "tool seed key mismatch"; exit 0; fi; if [ -d "$seed/mise/tree" ]; then /bin/mkdir -p "$HOME/.local/share/mise"; /bin/cp -R "$seed/mise/tree/." "$HOME/.local/share/mise/"; echo "tool seed restored share-dir"; fi; if [ -d "$seed/rustup/tree" ]; then /bin/mkdir -p "$RUNNER_TEMP/velnor/rustup"; /bin/cp -R "$seed/rustup/tree/." "$RUNNER_TEMP/velnor/rustup/"; echo "tool seed restored toolchain-dir"; fi"#
    )
}

/// Insert the seed step immediately before the setup at `setup_index`.
///
/// A second call leaves the existing seed step in place. A job with no
/// checkout before the setup stays without the local action. GitHub
/// cannot load `./.github/actions/velnor-tool-seed` until checkout runs.
///
/// # Errors
///
/// Returns [`RenderError`] when the setup step has no qualified cache key.
pub(crate) fn insert_before_setup(
    job: &mut Job,
    setup_index: usize,
    checkout_uses: &str,
    cache_key: &MiseToolsCacheKey,
) -> Result<usize, RenderError> {
    if setup_index >= job.steps.len() {
        return Err(RenderError::InvalidWorkflow("setup_missing".to_owned()));
    }
    let seed_indices = seed_indices(job);
    if seed_indices.len() > 1 {
        return Err(RenderError::InvalidWorkflow(
            "duplicate_tool_seed".to_owned(),
        ));
    }
    let Some(checkout_index) = checkout_before(job, setup_index, checkout_uses) else {
        if !seed_indices.is_empty() {
            return Err(RenderError::InvalidWorkflow(
                "tool_seed_without_configured_checkout".to_owned(),
            ));
        }
        return Ok(setup_index);
    };
    let expected = cache_key.as_str();
    if let Some(&seed_index) = seed_indices.first() {
        validate_seed_action(&job.steps[seed_index], Some(expected))?;
        if seed_index + 1 != setup_index || seed_index <= checkout_index {
            return Err(RenderError::InvalidWorkflow(
                "tool_seed_misordered".to_owned(),
            ));
        }
        return Ok(setup_index);
    }
    job.steps.insert(setup_index, seed_step(cache_key)?);
    Ok(setup_index + 1)
}

fn checkout_before(job: &Job, setup_index: usize, checkout_uses: &str) -> Option<usize> {
    job.steps[..setup_index]
        .iter()
        .position(|step| is_configured_checkout(step, checkout_uses))
}

/// Reject tool-seed steps in jobs that must not carry them.
/// # Errors
pub fn reject_orphan_seed(job_id: &str, job: &Job) -> Result<(), RenderError> {
    if seed_indices(job).is_empty() {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "tool_seed_without_setup:{job_id}"
        )))
    }
}

fn seed_indices(job: &Job) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| is_tool_seed_action(step).then_some(index))
        .collect()
}

fn is_tool_seed_action(step: &Step) -> bool {
    matches!(
        &step.kind,
        StepKind::Action { uses, .. } if uses == TOOL_SEED_USES
    )
}

/// Validate a tool-seed restore step against the expected cache key.
/// # Errors
pub fn validate_seed_action(step: &Step, expected_key: Option<&str>) -> Result<(), RenderError> {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "tool_seed_bad_action".to_owned(),
        ));
    };
    let key = with.get("cache_key");
    if uses != TOOL_SEED_USES
        || !is_tool_seed_step(step)
        || step.condition.is_some()
        || !env.is_empty()
        || with.len() != 1
        || key.is_none_or(|value| !crate::cache_p08::is_cache_key(value))
        || expected_key.is_some_and(|expected| key.map(String::as_str) != Some(expected))
    {
        return Err(RenderError::InvalidWorkflow(
            "tool_seed_bad_payload".to_owned(),
        ));
    }
    Ok(())
}

fn seed_step(cache_key: &MiseToolsCacheKey) -> Result<Step, RenderError> {
    let mut step = velnor_actions_workflow_steps::steps::action_step(
        TOOL_SEED_NAME,
        TOOL_SEED_USES,
        BTreeMap::from([("cache_key".to_owned(), cache_key.as_str().to_owned())]),
    )?;
    step.role = Some(StepRole::ToolSeed);
    Ok(step)
}

/// True when any job renders the tool-seed step.
#[must_use]
pub fn any_job_has_seed(jobs: &std::collections::BTreeMap<String, Job>) -> bool {
    jobs.values()
        .any(|job| job.steps.iter().any(is_tool_seed_step))
}

/// One composite action for every tool-seed step.
///
/// The workflow step stays a short `uses` plus the cache key. The copy
/// script lives here, outside the 500 KB workflow cap.
///
/// # Errors
///
/// Returns [`RenderError`] when the version or the script is invalid.
pub fn action_file(
    version: &str,
) -> Result<velnor_actions_workflow_tree::rendered::RenderedFile, RenderError> {
    let script = tool_seed_action_script(SEED_ROOT)?;
    let step = velnor_actions_workflow_steps::steps::shell_step(
        "Copy matching tool seed",
        vec!["bash".to_owned(), "-c".to_owned(), script],
        BTreeMap::from([("SEED_KEY".to_owned(), "${{ inputs.cache_key }}".to_owned())]),
    )?;
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow("tool_seed_step".to_owned()));
    };
    let body = action_yaml(
        &step.name,
        env,
        &velnor_actions_workflow_steps::commands::join_argv_for_run(run)?,
    );
    let quoted = velnor_actions_workflow_tree::yaml::quote_run_values_in_yaml(body);
    let bytes = velnor_actions_workflow_tree::marker::with_marker(
        version,
        &velnor_actions_workflow_tree::yaml::render_yaml(&quoted),
    )?;
    velnor_actions_workflow_steps::steps::scan_for_private_subcommands(&bytes)?;
    Ok(velnor_actions_workflow_tree::rendered::RenderedFile {
        path: TOOL_SEED_ACTION_PATH.to_owned(),
        bytes,
    })
}

fn action_yaml(step_name: &str, env: &BTreeMap<String, String>, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(TOOL_SEED_NAME.to_owned())),
        (
            "description".to_owned(),
            Yaml::str("Copy a matching host tool seed into this job.".to_owned()),
        ),
        (
            "inputs".to_owned(),
            Yaml::Map(vec![(
                "cache_key".to_owned(),
                Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        Yaml::str("Exact Mise cache key.".to_owned()),
                    ),
                    ("required".to_owned(), Yaml::Bool(true)),
                ]),
            )]),
        ),
        (
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite".to_owned())),
                (
                    "steps".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![
                        ("name".to_owned(), Yaml::str(step_name.to_owned())),
                        (
                            "env".to_owned(),
                            velnor_actions_workflow_tree::yaml::string_map_yaml(env),
                        ),
                        ("shell".to_owned(), Yaml::str("bash".to_owned())),
                        ("run".to_owned(), Yaml::str(run.to_owned())),
                    ])]),
                ),
            ]),
        ),
    ])
}

#[cfg(test)]
mod tests;
