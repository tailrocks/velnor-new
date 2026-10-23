//! Startup-safe rendering for the fixed release-verifier scripts.
//!
//! These steps run before repository code and inspect bytes produced by
//! earlier steps. Their shell startup, PATH, and credential boundary are
//! consequently fixed here rather than inherited from generic run rendering.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, VERIFY_PATH, VERIFY_VERIFIER_SHELL, commands, yaml::Yaml};

const VERIFY_RESTORED_RUST_STEP: &str = "Verify restored Rust toolchain";
const VERIFY_JACTIONLINT_STEP: &str = "Verify jactionlint";
const PROJECT_TOOLS_STEP: &str =
    velnor_actions_contract::config::project_tool_refs::PROJECT_TOOLS_STEP_NAME;
const ZIZMOR_GITHUB_ENV_COMMENT: &str = "zizmor: ignore[github-env]";
const INNER_SHELL: [&str; 7] = [
    "/bin/bash",
    "--noprofile",
    "--norc",
    "-p",
    "-e",
    "-o",
    "pipefail",
];
const FUNCTION_ENV_UNSET_PRELUDE: &str = r#"function_env_unset=(); function_env_scan_complete=0; while IFS= read -r -d '' env_entry; do case "$env_entry" in BASH_FUNC_*=*) function_env_unset+=(-u "${env_entry%%=*}");; VELNOR_VERIFIER_ENV_SCAN_COMPLETE) function_env_scan_complete=$((function_env_scan_complete + 1));; esac; done < <({ /usr/bin/env -0 && printf '%s\000' 'VELNOR_VERIFIER_ENV_SCAN_COMPLETE'; }); [ "$function_env_scan_complete" -eq 1 ] || exit 1"#;

#[derive(Clone, Copy)]
struct ScriptMetadata<'a> {
    id: Option<velnor_actions_contract::StepId>,
    condition: Option<&'a str>,
    run_comment: Option<&'a str>,
}

/// Render a typed verifier step, returning `None` for ordinary shell steps.
pub(crate) fn render_typed_step(
    step: &Step,
    run: &[String],
    env: &BTreeMap<String, String>,
    job_env: &BTreeMap<String, String>,
    composite: bool,
) -> Result<Option<Yaml>, RenderError> {
    let is_rust_name = matches!(
        step.name.as_str(),
        crate::VERIFY_RUST_TOOLCHAIN_STEP | VERIFY_RESTORED_RUST_STEP
    );
    let is_rust_role = step.role == Some(StepRole::VerifyRustManifest);
    let is_jactionlint_role = step.role == Some(StepRole::VerifyJactionlint);
    let is_jactionlint_name = step.name == VERIFY_JACTIONLINT_STEP;

    if !is_rust_role && !is_jactionlint_role {
        if is_rust_name || is_jactionlint_name {
            return Err(RenderError::InvalidWorkflow(
                "verifier_identity_mismatch".to_owned(),
            ));
        }
        return Ok(None);
    }

    if is_jactionlint_role {
        crate::selected_mise_verifier::validate_role_argv(run)?;
    } else {
        commands::validate_command_argv(run)?;
    }
    validate_verifier_env(env, composite)?;
    if run.len() != 3 || run[0] != "sh" || run[1] != "-c" {
        return Err(RenderError::InvalidWorkflow(
            "verifier_shell_shape_mismatch".to_owned(),
        ));
    }
    let unset = crate::toolchain_env::credential_unset_prelude();
    let expected_prefix = format!("{unset} ");
    let Some(script) = run[2].strip_prefix(&expected_prefix) else {
        return Err(RenderError::InvalidWorkflow(
            "verifier_credential_prelude_missing".to_owned(),
        ));
    };
    if script.starts_with(&expected_prefix) {
        return Err(RenderError::InvalidWorkflow(
            "verifier_shell_script_duplicate_credential_prelude".to_owned(),
        ));
    }

    render_script(
        &step.name,
        script,
        env,
        job_env,
        composite,
        ScriptMetadata {
            id: step.id,
            condition: step.condition.as_deref(),
            run_comment: None,
        },
    )
    .map(Some)
}

/// Render the fixed `sh -c SCRIPT` vector used by release workflow builders.
pub(crate) fn render_command(name: &str, argv: &[String]) -> Result<Yaml, RenderError> {
    let role = match name {
        crate::VERIFY_RUST_TOOLCHAIN_STEP => StepRole::VerifyRustManifest,
        VERIFY_JACTIONLINT_STEP => StepRole::VerifyJactionlint,
        _ => {
            return Err(RenderError::InvalidWorkflow(
                "unrecognized_release_verifier".to_owned(),
            ));
        }
    };
    let step = crate::verifier_composite::verifier_shell_step(
        role,
        name,
        argv.to_owned(),
        BTreeMap::new(),
    )?;
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "verifier_composite_requires_shell_step".to_owned(),
        ));
    };
    render_typed_step(&step, run, env, &BTreeMap::new(), false)?
        .ok_or_else(|| RenderError::InvalidWorkflow("verifier_role_required".to_owned()))
}

/// Render the generated project-tool resolver through its fixed verifier
/// boundary. This entry point is intentionally separate from display-name and
/// generic command routing.
pub(crate) fn render_project_tool_resolver(
    script: &str,
    composite: bool,
) -> Result<Yaml, RenderError> {
    render_script(
        PROJECT_TOOLS_STEP,
        script,
        &BTreeMap::new(),
        &BTreeMap::new(),
        composite,
        ScriptMetadata {
            id: None,
            condition: None,
            run_comment: Some(ZIZMOR_GITHUB_ENV_COMMENT),
        },
    )
}

/// Render a script after caller-specific validation has established its
/// fixed verifier identity and body.
fn render_script(
    name: &str,
    script: &str,
    base_env: &BTreeMap<String, String>,
    job_env: &BTreeMap<String, String>,
    composite: bool,
    metadata: ScriptMetadata<'_>,
) -> Result<Yaml, RenderError> {
    validate_verifier_env(base_env, composite)?;

    let mut env: BTreeMap<String, String> = base_env
        .iter()
        .filter(|(key, value)| job_env.get(*key).map(String::as_str) != Some(value.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    env.insert("PATH".to_owned(), VERIFY_PATH.to_owned());
    env.extend(crate::toolchain_env::credential_scrub());

    let mut inner_argv = crate::toolchain_env::with_env_unset_argv(&[]);
    inner_argv.extend(INNER_SHELL.iter().map(|arg| (*arg).to_owned()));
    inner_argv.push("-c".to_owned());
    let inner_argv = commands::join_argv_for_run(&inner_argv)?;
    let inner_argv = inner_argv
        .strip_prefix("env ")
        .ok_or_else(|| RenderError::InvalidWorkflow("verifier_env_prefix_missing".to_owned()))?;
    let prefix = format!(
        "{FUNCTION_ENV_UNSET_PRELUDE}; exec /usr/bin/env \"${{function_env_unset[@]}}\" -u BASH_ENV -u ENV {inner_argv}"
    );
    let inner_script = crate::toolchain_env::with_credential_unset_script(script);
    let run = format!("{prefix} {}", commands::quote_script_arg(&inner_script));

    let mut entries = vec![("name".to_owned(), Yaml::str(name.to_owned()))];
    if let Some(id) = metadata.id {
        crate::step_ids::push_explicit_step_id(&mut entries, id.as_str());
    }
    if let Some(condition) = metadata.condition {
        crate::steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.to_owned())));
    }
    entries.push((
        "env".to_owned(),
        Yaml::Map(
            env.into_iter()
                .map(|(key, value)| (key, Yaml::str(value)))
                .collect(),
        ),
    ));
    entries.push((
        "shell".to_owned(),
        Yaml::str(VERIFY_VERIFIER_SHELL.to_owned()),
    ));
    let run = match metadata.run_comment {
        Some(comment) => Yaml::annotated(run, comment),
        None => Yaml::str(run),
    };
    entries.push(("run".to_owned(), run));
    Ok(Yaml::Map(entries))
}

fn validate_verifier_env(
    env: &BTreeMap<String, String>,
    composite: bool,
) -> Result<(), RenderError> {
    if composite {
        commands::validate_composite_env(env)?;
    } else {
        commands::validate_env(env)?;
    }
    let scrub = crate::toolchain_env::credential_scrub();
    for (key, value) in env {
        if scrub.contains_key(key) && !value.is_empty() {
            return Err(RenderError::BadCommand(format!(
                "verifier_credential_not_empty:{key}"
            )));
        }
    }
    let ordinary: BTreeMap<String, String> = env
        .iter()
        .filter(|(key, _)| !scrub.contains_key(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    crate::toolchain_env::reject_denied_step_keys(&ordinary)
}

#[cfg(test)]
#[path = "verifier_shell_tests.rs"]
pub(crate) mod tests;
