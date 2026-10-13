//! Read-only tool seed at the fixed container path `/opt/velnor/seed`.
//!
//! A previous step cannot choose this path. A missing seed or a different
//! key stays cold. The job copies into its private homes. It does not
//! delete or write the seed.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::step_identity::is_tool_seed_step;
use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::yaml::Yaml;

use crate::tool_seed_admission::trusted_seed_guard;
pub(crate) use crate::tool_seed_admission::{SEED_ROOT, require_seed_root};
pub(crate) use velnor_actions_contract::workflow::step_identity::TOOL_SEED_USES;

/// Display name of the copy step ahead of `Setup Mise`.
pub(crate) const TOOL_SEED_NAME: &str = "Restore Velnor tool seed";
/// Repository path of that composite.
const TOOL_SEED_ACTION_PATH: &str = ".github/actions/velnor-tool-seed/action.yml";

/// Copy script. The composite sets `SEED_KEY` from `inputs.cache_key` and
/// exports `seed_admitted` only after the exact trusted seed was copied.
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
        r#"set -euo pipefail; seed="{seed_root}"; key={key_shell}; if [ -z "$key" ]; then echo "tool seed disabled"; exit 0; fi; {guard}; if [ ! -e "$seed" ]; then echo "tool seed absent"; exit 0; fi; if ! trusted_seed_is_trusted "$seed"; then echo "untrusted tool seed; continuing cold"; exit 0; fi; if [[ ! "$key" =~ ^mise-tools-v2-[0-9a-f]{{64}}$ ]] || [ ! -f "$seed/mise/KEY" ]; then echo "tool seed key mismatch"; exit 0; fi; if ! trusted_seed_file_matches "$seed/mise/KEY" "$key"; then echo "tool seed key mismatch"; exit 0; fi; trusted_seed_tree_has_file() {{ /usr/bin/find "$1" -type f -print -quit | /usr/bin/grep -q .; }}; copied=false; if [ -d "$seed/mise/tree" ] && trusted_seed_tree_has_file "$seed/mise/tree"; then /bin/mkdir -p "$HOME/.local/share/mise"; /bin/cp -R "$seed/mise/tree/." "$HOME/.local/share/mise/"; echo "tool seed restored share-dir"; copied=true; fi; if [ -d "$seed/rustup/tree" ] && trusted_seed_tree_has_file "$seed/rustup/tree"; then /bin/mkdir -p "$RUNNER_TEMP/velnor/rustup"; /bin/cp -R "$seed/rustup/tree/." "$RUNNER_TEMP/velnor/rustup/"; echo "tool seed restored toolchain-dir"; copied=true; fi; [ "$copied" = true ] || {{ echo "tool seed has no copyable payload"; exit 0; }}; printf 'seed_admitted=true\n' >> "$GITHUB_OUTPUT""#
    )
}

fn validate_seed_action(step: &Step) -> Result<(), RenderError> {
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
        || key.is_none_or(|value| !crate::cache_p08::is_v2_cache_key_expression(value))
    {
        return Err(RenderError::InvalidWorkflow(
            "tool_seed_bad_payload".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_action_call(
    step: &Step,
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    let StepKind::Action {
        uses: actual_uses,
        with: actual_with,
        env: actual_env,
    } = &step.kind
    else {
        return Err(RenderError::InvalidWorkflow(
            "tool_seed_bad_action".to_owned(),
        ));
    };
    if uses != actual_uses || with != actual_with || env != actual_env {
        return Err(RenderError::InvalidWorkflow(
            "tool_seed_bad_payload".to_owned(),
        ));
    }
    validate_seed_action(step)
}

/// Wrap a V2 cache key so workflow dispatch reads an empty key.
///
/// The guard expression evaluates to the inner key only when the trigger is
/// not `workflow_dispatch`.
pub(crate) fn guarded_seed_key(key: &str) -> String {
    format!("${{{{ github.event_name != 'workflow_dispatch' && '{key}' || '' }}}}")
}

/// True only for a dispatch-guarded V2 cache key.
pub(crate) fn is_guarded_seed_key(value: &str) -> bool {
    const PREFIX: &str = "${{ github.event_name != 'workflow_dispatch' && '";
    const SUFFIX: &str = "' || '' }}";
    value
        .strip_prefix(PREFIX)
        .and_then(|value| value.strip_suffix(SUFFIX))
        .is_some_and(crate::cache_p08::is_v2_cache_key_expression)
}

/// True when any job renders the tool-seed step.
pub(crate) fn any_job_has_seed(
    jobs: &std::collections::BTreeMap<String, Job>,
) -> Result<bool, RenderError> {
    let mut found = false;
    for job in jobs.values() {
        for step in &job.steps {
            let StepKind::Action { uses, .. } = &step.kind else {
                continue;
            };
            if uses != TOOL_SEED_USES {
                continue;
            }
            validate_seed_action(step)?;
            found = true;
        }
    }
    Ok(found)
}

/// One composite action for every tool-seed step.
///
/// The workflow step stays a short `uses` plus the cache key. The copy
/// script lives here, outside the 500 KB workflow cap.
///
/// # Errors
///
/// Returns [`RenderError`] when the version or the script is invalid.
pub(crate) fn action_file(version: &str) -> Result<crate::tree::RenderedFile, RenderError> {
    let script = tool_seed_action_script(SEED_ROOT)?;
    let step = crate::steps::shell_step(
        "Copy matching tool seed",
        vec!["bash".to_owned(), "-c".to_owned(), script],
        BTreeMap::from([("SEED_KEY".to_owned(), "${{ inputs.cache_key }}".to_owned())]),
    )?;
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow("tool_seed_step".to_owned()));
    };
    let body = action_yaml(&step.name, env, &crate::commands::join_argv_for_run(run)?);
    let bytes = crate::marker::with_marker(version, &crate::yaml::render_yaml(&body))?;
    crate::steps::scan_for_private_subcommands(&bytes)?;
    Ok(crate::tree::RenderedFile {
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
            "outputs".to_owned(),
            Yaml::Map(vec![(
                "seed_admitted".to_owned(),
                Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        Yaml::str("True only when the trusted seed matched and was copied."),
                    ),
                    (
                        "value".to_owned(),
                        Yaml::str("${{ steps.copy.outputs.seed_admitted }}"),
                    ),
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
                        ("id".to_owned(), Yaml::str("copy".to_owned())),
                        ("name".to_owned(), Yaml::str(step_name.to_owned())),
                        (
                            "env".to_owned(),
                            crate::document_steps::string_map_yaml(env),
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
#[path = "tool_seed_tests.rs"]
mod tests;
