//! Whole-template validation for action-owned MBX lifecycle commands.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, commands};

/// Resolve the executable installed by the pinned native action.
pub(crate) const RESOLVE_ACTION_MBX_BIN: &str = "if ! mbx_bin=$(command -v mbx); then printf '%s\\n' 'action-owned MBX is not on PATH' >&2; exit 1; fi; case \"$mbx_bin\" in /*) ;; *) printf '%s\\n' 'action-owned MBX path must be absolute' >&2; exit 1 ;; esac; if [ ! -f \"$mbx_bin\" ] || [ ! -x \"$mbx_bin\" ]; then printf '%s\\n' 'action-owned MBX path must be an executable file' >&2; exit 1; fi";

/// Shell-local absolute path assigned by [`RESOLVE_ACTION_MBX_BIN`].
pub(crate) const ACTION_MBX_BIN: &str = "$mbx_bin";

const RUST_EXEC_PREFIX: &str = "mise --no-config --no-env --no-hooks exec rust@";
const RUST_EXEC_QUOTED_PREFIX: &str = "mise --no-config --no-env --no-hooks exec 'rust@'\"";
const VERSION_LINE_PREFIX: &str = "[ \"$version_line\" = 'mbx ";
const VERSION_LINE_SUFFIX: &str = "' ] ||";
const PRESEED_VERSION_PREFIX: &str = "grep -qxF \"mbx ";
const PRESEED_VERSION_SUFFIX: &str = "\"";

/// Validate a fixed internally-built shell vector before its role is attached.
/// The serialized form is checked again against a role-specific producer.
pub(crate) fn validate_argv(argv: &[String]) -> Result<(), RenderError> {
    let script_at =
        commands::inline_shell_script_at(argv).ok_or_else(|| bad_shape("not_inline_shell"))?;
    let script = &argv[script_at];
    if script.matches(RESOLVE_ACTION_MBX_BIN).count() != 1 || !script.contains(" -- \"$mbx_bin\"") {
        return Err(bad_shape("resolver_or_binary_binding"));
    }

    let mut validated = argv.to_vec();
    validated[script_at] = script.replace(RESOLVE_ACTION_MBX_BIN, "ACTION_MBX_RESOLVER");
    commands::validate_command_argv(&validated)
}

/// Validate a serialized MBX role by rebuilding its complete canonical argv.
pub(crate) fn validate_argv_for_role(
    argv: &[String],
    role: Option<StepRole>,
) -> Result<(), RenderError> {
    if validate_bundle_argv(argv, role)? {
        return Ok(());
    }
    validate_argv(argv)?;
    let script_at =
        commands::inline_shell_script_at(argv).ok_or_else(|| bad_shape("not_inline_shell"))?;
    let script = &argv[script_at];
    let rust = rust_toolchain(script)?;
    let expected = match role {
        Some(StepRole::MbxVersionCheck) => {
            let version = value_between(script, VERSION_LINE_PREFIX, VERSION_LINE_SUFFIX)?;
            shell_run(crate::cache_steps::mbx_version_check_step(
                version,
                rust,
                BTreeMap::new(),
            )?)?
        }
        Some(StepRole::MbxWorkspaceCleanup) => {
            let mut with = BTreeMap::from([
                ("version".to_owned(), "1.0.0".to_owned()),
                ("toolchain".to_owned(), rust.to_owned()),
            ]);
            if let Some((_, after)) = script.split_once("; target_dir=\"")
                && let Some((td, _)) = after.split_once('"')
                && let Some(dir) = td.strip_suffix("/target")
            {
                with.insert("working-directory".to_owned(), dir.to_owned());
            }
            let action = Step {
                parallel: false,
                name: crate::cache_steps::MBX_RESTORE_NAME.to_owned(),
                id: None,
                role: Some(StepRole::MbxCache),
                condition: None,
                kind: StepKind::Action {
                    uses: format!("{}@{}", crate::cache_steps::MBX_ACTION_NAME, "a".repeat(40)),
                    with,
                    env: BTreeMap::new(),
                },
            };
            shell_run(crate::cache_steps::mbx_workspace_clean_step_for_action(
                &action,
            )?)?
        }
        Some(StepRole::PreseedBuild) => {
            let with_toolchain = script.contains(&format!("+{rust}"));
            let build = candidate_build_argv(rust, with_toolchain);
            shell_run(crate::preseed::preseed_build_step(
                &build,
                &BTreeMap::new(),
            )?)?
        }
        Some(StepRole::PreseedVerifyBuild) => {
            let version = value_between(script, PRESEED_VERSION_PREFIX, PRESEED_VERSION_SUFFIX)?;
            let probe = mbx_probe_argv(rust);
            shell_run(crate::preseed::preseed_verify_step(
                &probe,
                version,
                &BTreeMap::new(),
            )?)?
        }
        _ => return Err(bad_shape("role_not_authorized")),
    };
    if argv != expected {
        return Err(bad_shape("canonical_template_mismatch"));
    }
    Ok(())
}

fn validate_bundle_argv(argv: &[String], role: Option<StepRole>) -> Result<bool, RenderError> {
    let mode = match role {
        Some(StepRole::MbxBundleImport) => "import",
        Some(StepRole::MbxBundleExport) => "export",
        _ => return Ok(false),
    };
    let expected_step = match mode {
        "import" => crate::cache_steps::mbx_bundle_import_step("1.99.0", None, BTreeMap::new())?,
        "export" => crate::cache_steps::mbx_bundle_export_step("1.99.0", None, BTreeMap::new())?,
        _ => return Err(bad_shape("unknown_bundle_mode")),
    };
    if argv != shell_run(expected_step)? {
        return Err(bad_shape("canonical_bundle_template_mismatch"));
    }
    Ok(true)
}

/// Validate a shell step using its role when the role owns an MBX template.
///
/// All other steps retain the generic fixed-argv validator. A role never
/// grants a partial shell exception: MBX roles must match a complete
/// producer-generated template in [`validate_argv_for_role`].
pub(crate) fn validate_step_argv(
    argv: &[String],
    role: Option<StepRole>,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if matches!(
        role,
        Some(StepRole::MbxBundleImport | StepRole::MbxBundleExport)
    ) {
        validate_bundle_environment(role, env)?;
    }
    if role_uses_canonical_template(role) {
        validate_argv_for_role(argv, role)
    } else {
        commands::validate_command_argv(argv)
    }
}

fn validate_bundle_environment(
    role: Option<StepRole>,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    let root = env.get("VELNOR_MBX_BUNDLE_ROOT").map(String::as_str);
    let rust_toolchain = env
        .get("VELNOR_MBX_BUNDLE_RUST_TOOLCHAIN")
        .map_or("1.99.0", String::as_str);
    let step = match role {
        Some(StepRole::MbxBundleImport) => {
            crate::cache_steps::mbx_bundle_import_step(rust_toolchain, root, BTreeMap::new())?
        }
        Some(StepRole::MbxBundleExport) => {
            crate::cache_steps::mbx_bundle_export_step(rust_toolchain, root, BTreeMap::new())?
        }
        _ => return Err(bad_shape("bundle_environment_without_bundle_role")),
    };
    let StepKind::Shell {
        env: expected_env, ..
    } = step.kind
    else {
        return Err(bad_shape("canonical_bundle_step_not_shell"));
    };
    if expected_env
        .get("VELNOR_MBX_BUNDLE_ROOT")
        .map(String::as_str)
        != root
        || env.get("VELNOR_MBX_BUNDLE_RUST_TOOLCHAIN")
            != expected_env.get("VELNOR_MBX_BUNDLE_RUST_TOOLCHAIN")
    {
        return Err(bad_shape("canonical_bundle_environment_mismatch"));
    }
    Ok(())
}

/// Whether this role selects one of the complete MBX templates.
fn role_uses_canonical_template(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::MbxVersionCheck
                | StepRole::MbxWorkspaceCleanup
                | StepRole::MbxBundleImport
                | StepRole::MbxBundleExport
                | StepRole::PreseedBuild
                | StepRole::PreseedVerifyBuild
        )
    )
}

fn rust_toolchain(script: &str) -> Result<&str, RenderError> {
    // Project-declared Rust renders `'rust@'"${VELNOR_TOOL_RUST}"` (hard decision 5).
    if let Some((_, after)) = script.split_once(RUST_EXEC_QUOTED_PREFIX)
        && let Some((rust, _)) = after.split_once("\" -- ")
        && velnor_actions_contract::config::project_tool_refs::is_project_tool_ref(rust)
    {
        return Ok(rust);
    }
    let (_, after) = script
        .split_once(RUST_EXEC_PREFIX)
        .ok_or_else(|| bad_shape("rust_toolchain_missing"))?;
    let (rust, _) = after
        .split_once(" -- ")
        .ok_or_else(|| bad_shape("rust_toolchain_unclosed"))?;
    if !crate::commands::is_exact_or_project_version(rust) {
        return Err(bad_shape("rust_toolchain_invalid"));
    }
    Ok(rust)
}

fn value_between<'a>(script: &'a str, prefix: &str, suffix: &str) -> Result<&'a str, RenderError> {
    if script.matches(prefix).count() != 1 {
        return Err(bad_shape("dynamic_value_not_unique"));
    }
    let (_, after) = script
        .split_once(prefix)
        .ok_or_else(|| bad_shape("dynamic_value_missing"))?;
    let (value, _) = after
        .split_once(suffix)
        .ok_or_else(|| bad_shape("dynamic_value_unclosed"))?;
    if !crate::commands::is_exact_or_project_version(value) {
        return Err(bad_shape("dynamic_value_invalid"));
    }
    Ok(value)
}

fn candidate_build_argv(rust: &str, with_toolchain: bool) -> Vec<String> {
    let mut argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        format!("rust@{rust}"),
        "--".to_owned(),
        "mbx".to_owned(),
    ];
    if with_toolchain {
        argv.push(format!("+{rust}"));
    }
    argv.extend([
        "build".to_owned(),
        "--release".to_owned(),
        "--locked".to_owned(),
        "--package".to_owned(),
        "velnor-actions-cli".to_owned(),
        "--bin".to_owned(),
        "velnor-actions".to_owned(),
    ]);
    argv
}

fn mbx_probe_argv(rust: &str) -> Vec<String> {
    let argv = [
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        format!("rust@{rust}"),
        "--".to_owned(),
        "mbx".to_owned(),
        "--version".to_owned(),
    ];
    argv.to_vec()
}

fn shell_run(step: Step) -> Result<Vec<String>, RenderError> {
    match step.kind {
        StepKind::Shell { run, .. } => Ok(run),
        _ => Err(bad_shape("canonical_step_not_shell")),
    }
}

pub(crate) fn target_dir_for_action(
    with: &BTreeMap<String, String>,
) -> Result<String, RenderError> {
    let Some(dir) = with
        .get("working-directory")
        .map(String::as_str)
        .filter(|d| !d.is_empty() && *d != ".")
    else {
        return Ok("target".to_owned());
    };
    if !dir.starts_with('/')
        && !dir.ends_with('/')
        && dir.split('/').all(|s| {
            !s.is_empty()
                && s != "."
                && s != ".."
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
    {
        Ok(format!("{dir}/target"))
    } else {
        Err(RenderError::BadCommand(format!("bad_working_dir:{dir}")))
    }
}

fn bad_shape(reason: &str) -> RenderError {
    RenderError::BadCommand(format!("action_owned_mbx_binding:{reason}"))
}

#[cfg(test)]
#[path = "action_mbx_shell_tests.rs"]
mod tests;
